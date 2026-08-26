mod audio;
mod capture;
mod db;
mod error;
mod extract;
mod image_util;
mod models;
mod permissions;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::audio::Recorder;
use crate::db::Db;
use crate::error::SnagError;
use crate::models::{
    AppSettings, CaptureBundle, PermissionStatus, SessionState, Task, TaskPatch, TaskStatus,
};

struct LiveSession {
    capture: Option<CaptureBundle>,
    recorder: Option<Recorder>,
    phase: String,
    cancel: bool,
}

impl LiveSession {
    fn idle() -> Self {
        Self {
            capture: None,
            recorder: None,
            phase: "idle".into(),
            cancel: false,
        }
    }
}

struct AppState {
    db: Db,
    live: Mutex<LiveSession>,
    _demo_hotkey: Mutex<String>,
}

fn emit_session(app: &AppHandle, state: SessionState) {
    let _ = app.emit("snag://session", state);
}

fn emit_tasks(app: &AppHandle) {
    let _ = app.emit("snag://tasks", ());
}

fn show_overlay(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn hide_overlay(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.hide();
    }
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
fn list_tasks(state: State<AppState>) -> Result<Vec<Task>, SnagError> {
    state.db.list_tasks()
}

#[tauri::command]
fn upsert_task(state: State<AppState>, app: AppHandle, task: Task) -> Result<(), SnagError> {
    state.db.insert_task(&task)?;
    emit_tasks(&app);
    Ok(())
}

#[tauri::command]
fn update_task(state: State<AppState>, app: AppHandle, id: String, patch: TaskPatch) -> Result<Task, SnagError> {
    let t = state.db.update_task(&id, patch)?;
    emit_tasks(&app);
    Ok(t)
}

#[tauri::command]
fn delete_task(state: State<AppState>, app: AppHandle, id: String) -> Result<(), SnagError> {
    state.db.delete_task(&id)?;
    emit_tasks(&app);
    Ok(())
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Result<AppSettings, SnagError> {
    state.db.get_settings()
}

#[tauri::command]
fn save_settings(state: State<AppState>, app: AppHandle, settings: AppSettings) -> Result<(), SnagError> {
    state.db.save_settings(&settings)?;
    reregister_hotkey(&app, &settings.hotkey);
    Ok(())
}

#[tauri::command]
fn check_permissions() -> PermissionStatus {
    permissions::status()
}

#[tauri::command]
fn request_permissions() -> PermissionStatus {
    permissions::request()
}

#[tauri::command]
fn acknowledge_permissions(state: State<AppState>) -> Result<(), SnagError> {
    let mut s = state.db.get_settings()?;
    s.permissions_explained = true;
    state.db.save_settings(&s)
}

fn reregister_hotkey(app: &AppHandle, hotkey: &str) {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let key = hotkey.trim();
    if key.is_empty() {
        return;
    }
    if let Err(err) = gs.on_shortcut(key.to_string(), |app, _shortcut, event| {
        if event.state != ShortcutState::Pressed {
            return;
        }
        let _ = handle_hotkey(app);
    }) {
        log::warn!("hotkey register failed: {err}");
    }
}

fn handle_hotkey(app: &AppHandle) -> Result<(), SnagError> {
    let state = app.state::<AppState>();
    let phase = state.live.lock().expect("live").phase.clone();
    if phase == "listening" {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = finish_capture(app).await;
        });
        return Ok(());
    }
    if phase == "processing" {
        return Ok(());
    }
    start_capture_inner(app)
}

#[tauri::command]
fn start_capture(app: AppHandle) -> Result<(), SnagError> {
    start_capture_inner(&app)
}

fn start_capture_inner(app: &AppHandle) -> Result<(), SnagError> {
    let state = app.state::<AppState>();
    let settings = state.db.get_settings()?;
    if !settings.demo_mode && !settings.permissions_explained && cfg!(target_os = "macos") {
        emit_session(app, SessionState::explain());
        show_main(app);
        return Ok(());
    }

    let bundle = capture::capture(settings.demo_mode, &settings.demo_fixture)?;
    let recorder = if settings.demo_mode {
        None
    } else {
        match Recorder::start() {
            Ok(r) => Some(r),
            Err(err) => {
                log::warn!("mic: {err}");
                None
            }
        }
    };

    {
        let mut live = state.live.lock().expect("live");
        *live = LiveSession {
            capture: Some(bundle),
            recorder,
            phase: "listening".into(),
            cancel: false,
        };
    }

    emit_session(app, SessionState::listening());
    show_overlay(app);

    let app_handle = app.clone();
    let demo = settings.demo_mode;
    tauri::async_runtime::spawn(async move {
        if demo {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let st = app_handle.state::<AppState>();
            let phase = st.live.lock().expect("live").phase.clone();
            if phase == "listening" {
                let _ = finish_capture(app_handle).await;
            }
            return;
        }
        loop {
            tokio::time::sleep(Duration::from_millis(180)).await;
            let st = app_handle.state::<AppState>();
            let should = {
                let live = st.live.lock().expect("live");
                if live.phase != "listening" || live.cancel {
                    false
                } else {
                    live.recorder
                        .as_ref()
                        .map(|r| r.should_autostop())
                        .unwrap_or(false)
                }
            };
            if should {
                let _ = finish_capture(app_handle).await;
                break;
            }
            let phase = st.live.lock().expect("live").phase.clone();
            if phase != "listening" {
                break;
            }
        }
    });
    Ok(())
}

#[tauri::command]
async fn stop_capture(app: AppHandle) -> Result<(), SnagError> {
    finish_capture(app).await
}

async fn finish_capture(app: AppHandle) -> Result<(), SnagError> {
    let state = app.state::<AppState>();
    let (bundle, recorder, cancelled, settings) = {
        let mut live = state.live.lock().expect("live");
        if live.phase != "listening" {
            return Ok(());
        }
        live.phase = "processing".into();
        let bundle = live.capture.take();
        let recorder = live.recorder.take();
        let cancelled = live.cancel;
        (bundle, recorder, cancelled, state.db.get_settings()?)
    };
    if cancelled {
        emit_session(&app, SessionState::idle());
        hide_overlay(&app);
        *state.live.lock().expect("live") = LiveSession::idle();
        return Ok(());
    }
    let Some(bundle) = bundle else {
        emit_session(&app, SessionState::error("Nothing was captured"));
        hide_overlay(&app);
        *state.live.lock().expect("live") = LiveSession::idle();
        return Ok(());
    };

    emit_session(&app, SessionState::processing());
    show_overlay(&app);

    let mut transcript = String::new();
    if settings.demo_mode {
        transcript = "add this as a task for me".into();
    } else if let Some(rec) = recorder {
        let wav = rec.stop_wav().unwrap_or_default();
        if !settings.api_key.is_empty() && settings.provider.as_str() == "openai" {
            match extract::transcribe_openai(&settings.api_key, &wav).await {
                Ok(t) => transcript = t,
                Err(err) => log::warn!("transcribe: {err}"),
            }
        }
        if transcript.trim().is_empty() {
            transcript = "add this as a task for me".into();
        }
        let _ = wav; // discarded — never persisted
    } else {
        transcript = "add this as a task for me".into();
    }

    let extracted = extract::extract(&settings, &bundle, &transcript).await;
    drop(bundle); // screenshots discarded

    let now = chrono::Utc::now().to_rfc3339();
    let task = Task {
        id: uuid::Uuid::new_v4().to_string(),
        title: extracted.title.clone(),
        notes: extracted.notes,
        status: TaskStatus::Inbox,
        due_hint: extracted.due_hint,
        source_app: extracted.source_app,
        source_window: bundle.window_title.clone(),
        confidence: Some(extracted.confidence),
        created_at: now.clone(),
        updated_at: now,
        completed_at: None,
    };
    state.db.insert_task(&task)?;
    emit_tasks(&app);
    emit_session(&app, SessionState::done(task.title));
    tokio::time::sleep(Duration::from_millis(1400)).await;
    emit_session(&app, SessionState::idle());
    hide_overlay(&app);
    *state.live.lock().expect("live") = LiveSession::idle();
    Ok(())
}

#[tauri::command]
fn cancel_capture(app: AppHandle) -> Result<(), SnagError> {
    let state = app.state::<AppState>();
    {
        let mut live = state.live.lock().expect("live");
        live.cancel = true;
        live.phase = "idle".into();
        live.capture = None;
        live.recorder = None;
    }
    emit_session(&app, SessionState::idle());
    hide_overlay(&app);
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let dir = app.path().app_data_dir().expect("app data dir");
            let db = Db::open(&dir.join("snag.sqlite"))?;
            let settings = db.get_settings().unwrap_or_default();
            let hotkey = settings.hotkey.clone();
            app.manage(AppState {
                db,
                live: Mutex::new(LiveSession::idle()),
                demo_hotkey: Mutex::new(hotkey.clone()),
            });

            let show = MenuItem::with_id(app, "show", "Show Inbox", true, None::<&str>)?;
            let snag = MenuItem::with_id(app, "snag", "Snag from screen", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Snag", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&snag, &show, &quit])?;

            let _ = TrayIconBuilder::new()
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "snag" => {
                        let _ = start_capture_inner(app);
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                })
                .build(app);

            reregister_hotkey(&app.handle(), &hotkey);
            let _ = Arc::new(());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_tasks,
            upsert_task,
            update_task,
            delete_task,
            get_settings,
            save_settings,
            start_capture,
            stop_capture,
            cancel_capture,
            check_permissions,
            request_permissions,
            acknowledge_permissions
        ])
        .run(tauri::generate_context!())
        .expect("error while running Snag");
}
