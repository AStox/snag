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
    if let Err(err) = gs.on_shortcut(key, |app, _shortcut, event| {
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
    // Second hotkey during processing is ignored. No listen/stop toggle.
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
    {
        let live = state.live.lock().expect("live");
        if live.phase == "processing" {
            return Ok(());
        }
    }
    let settings = state.db.get_settings()?;
    if !settings.demo_mode && !settings.permissions_explained && cfg!(target_os = "macos") {
        emit_session(app, SessionState::explain());
        show_main(app);
        return Ok(());
    }

    let bundle = capture::capture(settings.demo_mode, &settings.demo_fixture)?;
    // Default flow: no mic. Recorder stays on LiveSession so audio.rs keeps compiling.

    {
        let mut live = state.live.lock().expect("live");
        *live = LiveSession {
            capture: Some(bundle),
            recorder: None,
            phase: "processing".into(),
            cancel: false,
        };
    }

    emit_session(app, SessionState::processing());
    show_overlay(app);

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = finish_capture(app_handle).await;
    });
    Ok(())
}

#[tauri::command]
async fn stop_capture(app: AppHandle) -> Result<(), SnagError> {
    finish_capture(app).await
}

async fn finish_capture(app: AppHandle) -> Result<(), SnagError> {
    let state = app.state::<AppState>();
    let (bundle, cancelled, settings) = {
        let mut live = state.live.lock().expect("live");
        if live.phase != "processing" {
            return Ok(());
        }
        let bundle = live.capture.take();
        let cancelled = live.cancel;
        // Drop any leftover recorder without using it — default flow never starts the mic.
        let _recorder = live.recorder.take();
        (bundle, cancelled, state.db.get_settings()?)
    };
    if cancelled {
        emit_session(&app, SessionState::idle());
        hide_overlay(&app);
        *state.live.lock().expect("live") = LiveSession::idle();
        return Ok(());
    }
    // Missing bundle while still processing means another finish is already in flight.
    let Some(bundle) = bundle else {
        return Ok(());
    };

    emit_session(&app, SessionState::processing());
    show_overlay(&app);

    // Short beat so the overlay is readable (demo has no model latency).
    tokio::time::sleep(Duration::from_millis(380)).await;
    if aborted(&app) {
        settle_idle(&app);
        return Ok(());
    }

    let window_title = bundle.window_title.clone();
    let extracted = extract::extract(&settings, &bundle, "").await;
    drop(bundle); // screenshots discarded

    if aborted(&app) {
        settle_idle(&app);
        return Ok(());
    }

    let filed: Vec<_> = extracted.into_iter().filter(extract::should_file).collect();
    if filed.is_empty() {
        {
            let mut live = state.live.lock().expect("live");
            live.phase = "done".into();
        }
        emit_session(&app, SessionState::done("Nothing to snag".into()));
        tokio::time::sleep(Duration::from_millis(1400)).await;
        settle_idle(&app);
        return Ok(());
    }

    let now = chrono::Utc::now().to_rfc3339();
    let overlay = extract::overlay_title(&filed);
    for item in filed {
        let task = Task {
            id: uuid::Uuid::new_v4().to_string(),
            title: item.title,
            notes: item.notes,
            status: TaskStatus::Inbox,
            due_hint: item.due_hint,
            source_app: item.source_app,
            source_window: window_title.clone(),
            confidence: Some(item.confidence),
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: None,
        };
        state.db.insert_task(&task)?;
    }
    emit_tasks(&app);
    {
        let mut live = state.live.lock().expect("live");
        live.phase = "done".into();
    }
    emit_session(&app, SessionState::done(overlay));
    tokio::time::sleep(Duration::from_millis(1400)).await;
    settle_idle(&app);
    Ok(())
}

fn aborted(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let live = state.live.lock().expect("live");
    live.cancel || live.phase != "processing"
}

fn settle_idle(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut live = state.live.lock().expect("live");
    if live.phase == "done" {
        *live = LiveSession::idle();
        drop(live);
        emit_session(app, SessionState::idle());
        hide_overlay(app);
    }
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
