mod audio;
mod capture;
mod db;
mod debug_loop;
mod error;
mod extract;
mod image_util;
mod models;
mod permissions;

use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::audio::Recorder;
use crate::db::Db;
use crate::error::SnagError;
use crate::models::{
    AppSettings, CaptureBundle, PermissionStatus, Provider, SessionState, Task, TaskPatch,
    TaskStatus,
};

struct LiveSession {
    capture: Option<CaptureBundle>,
    recorder: Option<Recorder>,
    dump: Option<debug_loop::SessionDump>,
    phase: String,
    cancel: bool,
}

impl LiveSession {
    fn idle() -> Self {
        Self {
            capture: None,
            recorder: None,
            dump: None,
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

fn place_overlay(app: &AppHandle) {
    let Some(w) = app.get_webview_window("overlay") else {
        return;
    };
    let _ = w.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
    let monitor = w
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| w.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let size = monitor.size();
    let scale = monitor.scale_factor();
    let lw = 380.0;
    let lh = 76.0;
    let x = ((size.width as f64 / scale) - lw) / 2.0;
    let y = (size.height as f64 / scale) - lh - 28.0;
    let _ = w.set_size(tauri::LogicalSize::new(lw, lh));
    let _ = w.set_position(tauri::LogicalPosition::new(x.max(0.0), y.max(0.0)));
}

fn show_overlay(app: &AppHandle) {
    place_overlay(app);
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.show();
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

fn refresh_status(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.db.get_settings().unwrap_or_default();
    let perms = permissions::status();
    let phase = state.live.lock().expect("live").phase.clone();
    debug_loop::write_status(&json!({
        "ok": true,
        "phase": phase,
        "hotkey": settings.hotkey,
        "demoMode": settings.demo_mode,
        "hasApiKey": !settings.api_key.trim().is_empty(),
        "provider": settings.provider.as_str(),
        "model": settings.model,
        "permissionsExplained": settings.permissions_explained,
        "permissions": {
            "screen": perms.screen,
            "accessibility": perms.accessibility,
            "microphone": perms.microphone,
        },
        "control": "http://127.0.0.1:17333",
        "dir": debug_loop::root().display().to_string(),
    }));
}

#[tauri::command]
fn open_provider_console(provider: String) -> Result<(), SnagError> {
    let p = match provider.as_str() {
        "openai" => Provider::Openai,
        "anthropic" => Provider::Anthropic,
        "xai" => Provider::Xai,
        _ => return Err(SnagError::from("unknown provider")),
    };
    let url = p.console_url();
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| SnagError::from(e.to_string()))?;
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = url;
        Err(SnagError::from("opening the console is macOS-only"))
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
    refresh_status(&app);
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
    let key = hotkey.trim().to_string();
    if key.is_empty() {
        debug_loop::event("hotkey_skip", json!({ "reason": "empty" }));
        return;
    }
    let key_cb = key.clone();
    match gs.on_shortcut(key.as_str(), move |app, _shortcut, event| {
        if event.state != ShortcutState::Pressed {
            return;
        }
        debug_loop::event("hotkey", json!({ "hotkey": key_cb }));
        let _ = handle_hotkey(app);
    }) {
        Ok(_) => debug_loop::event("hotkey_ok", json!({ "hotkey": key })),
        Err(err) => debug_loop::event("hotkey_fail", json!({ "hotkey": key, "error": err.to_string() })),
    }
}

fn handle_hotkey(app: &AppHandle) -> Result<(), SnagError> {
    let state = app.state::<AppState>();
    let phase = state.live.lock().expect("live").phase.clone();
    if phase == "processing" {
        debug_loop::event("hotkey_ignored", json!({ "phase": phase }));
        return Ok(());
    }
    start_capture_opts(app, false, "hotkey")
}

#[tauri::command]
fn start_capture(app: AppHandle) -> Result<(), SnagError> {
    start_capture_opts(&app, false, "button")
}

fn start_capture_opts(app: &AppHandle, skip_explain: bool, trigger: &str) -> Result<(), SnagError> {
    debug_loop::event(
        "capture_requested",
        json!({ "skip_explain": skip_explain, "trigger": trigger }),
    );
    match start_capture_opts_impl(app, skip_explain, trigger) {
        Ok(()) => Ok(()),
        Err(e) => {
            debug_loop::event(
                "capture_error",
                json!({ "error": e.to_string(), "trigger": trigger }),
            );
            let dump = debug_loop::SessionDump::begin(trigger);
            dump.finish(json!({
                "ok": false,
                "error": e.to_string(),
                "trigger": trigger,
            }));
            emit_session(app, SessionState::error(e.to_string()));
            show_overlay(app);
            {
                let state = app.state::<AppState>();
                let mut live = state.live.lock().expect("live");
                live.phase = "error".into();
            }
            refresh_status(app);
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(2800)).await;
                settle_idle(&app2);
            });
            Err(e)
        }
    }
}

fn start_capture_opts_impl(
    app: &AppHandle,
    skip_explain: bool,
    trigger: &str,
) -> Result<(), SnagError> {
    let state = app.state::<AppState>();
    {
        let live = state.live.lock().expect("live");
        if live.phase == "processing" {
            return Ok(());
        }
    }
    let mut settings = state.db.get_settings()?;
    if skip_explain && !settings.permissions_explained {
        settings.permissions_explained = true;
        state.db.save_settings(&settings)?;
    }
    if !skip_explain && !settings.demo_mode && !settings.permissions_explained && cfg!(target_os = "macos")
    {
        debug_loop::event("capture_blocked_explain", json!({ "trigger": trigger }));
        debug_loop::write_json(
            &debug_loop::root().join("last.json"),
            &json!({
                "ok": false,
                "blocked": "permissions_explain",
                "hint": "First capture opens the permissions explainer. POST /ack-perms then POST /capture.",
                "trigger": trigger,
            }),
        );
        emit_session(app, SessionState::explain());
        show_main(app);
        refresh_status(app);
        return Ok(());
    }

    {
        let mut live = state.live.lock().expect("live");
        live.phase = "processing".into();
        live.cancel = false;
    }
    emit_session(app, SessionState::processing());
    show_overlay(app);

    if !settings.demo_mode {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.hide();
        }
        // Let the compositor drop the inbox so the shot is whatever is under the cursor.
        std::thread::sleep(Duration::from_millis(16));
    }
    let bundle = capture::capture(settings.demo_mode, &settings.demo_fixture)?;
    let perms = permissions::status();
    let dump = debug_loop::SessionDump::begin(trigger);
    dump.write_capture(&bundle, &settings, &perms);

    {
        let mut live = state.live.lock().expect("live");
        *live = LiveSession {
            capture: Some(bundle),
            recorder: None,
            dump: Some(dump),
            phase: "processing".into(),
            cancel: false,
        };
    }

    show_overlay(app);
    refresh_status(app);

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
    let (bundle, cancelled, settings, dump) = {
        let mut live = state.live.lock().expect("live");
        if live.phase != "processing" {
            return Ok(());
        }
        let bundle = live.capture.take();
        let cancelled = live.cancel;
        let dump = live.dump.take();
        let _recorder = live.recorder.take();
        (bundle, cancelled, state.db.get_settings()?, dump)
    };
    if cancelled {
        if let Some(d) = dump {
            d.finish(json!({ "ok": false, "cancelled": true }));
        }
        emit_session(&app, SessionState::idle());
        hide_overlay(&app);
        *state.live.lock().expect("live") = LiveSession::idle();
        refresh_status(&app);
        return Ok(());
    }
    let Some(bundle) = bundle else {
        return Ok(());
    };

    emit_session(&app, SessionState::processing());
    show_overlay(&app);

    let window_title = bundle.window_title.clone();
    let report = extract::extract(&settings, &bundle, "").await;
    if let Some(d) = dump.as_ref() {
        let mut v = serde_json::to_value(&report).unwrap_or(json!({}));
        if let Some(obj) = v.as_object_mut() {
            obj.insert("taskCount".into(), json!(report.task_count));
        }
        d.write_extract(&v);
    }
    drop(bundle);

    if aborted(&app) {
        settle_idle(&app);
        return Ok(());
    }

    let filed: Vec<_> = report
        .tasks
        .into_iter()
        .filter(extract::should_file)
        .collect();
    if filed.is_empty() {
        {
            let mut live = state.live.lock().expect("live");
            live.phase = "done".into();
        }
        if let Some(d) = dump {
            d.finish(json!({
                "ok": true,
                "overlay": "Nothing to snag",
                "filed": 0,
                "path": report.path,
                "error": report.error,
            }));
        }
        emit_session(&app, SessionState::done("Nothing to snag".into()));
        tokio::time::sleep(Duration::from_millis(450)).await;
        settle_idle(&app);
        return Ok(());
    }

    let now = chrono::Utc::now().to_rfc3339();
    let overlay = extract::overlay_title(&filed);
    let titles: Vec<String> = filed.iter().map(|t| t.title.clone()).collect();
    let n = filed.len();
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
    if let Some(d) = dump {
        d.finish(json!({
            "ok": true,
            "overlay": overlay,
            "filed": n,
            "titles": titles,
            "path": report.path,
            "error": report.error,
        }));
    }
    emit_session(&app, SessionState::done(overlay));
    tokio::time::sleep(Duration::from_millis(450)).await;
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
    if live.phase == "done" || live.phase == "error" {
        *live = LiveSession::idle();
        drop(live);
        emit_session(app, SessionState::idle());
        hide_overlay(app);
        show_main(app);
        refresh_status(app);
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
        live.dump = None;
    }
    emit_session(&app, SessionState::idle());
    hide_overlay(&app);
    refresh_status(&app);
    Ok(())
}

fn spawn_command_poller(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let Some(cmd) = debug_loop::take_command() else {
                continue;
            };
            let op = cmd.get("op").and_then(|v| v.as_str()).unwrap_or("");
            debug_loop::event("command", cmd.clone());
            let result = match op {
                "capture" => {
                    let skip = cmd
                        .get("skip_explain")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    match start_capture_opts(&app, skip, "control") {
                        Ok(()) => json!({ "ok": true, "op": "capture" }),
                        Err(e) => json!({ "ok": false, "op": "capture", "error": e.to_string() }),
                    }
                }
                "request_perms" => {
                    let p = permissions::request();
                    refresh_status(&app);
                    json!({
                        "ok": true,
                        "op": "request_perms",
                        "permissions": {
                            "screen": p.screen,
                            "accessibility": p.accessibility,
                            "microphone": p.microphone
                        }
                    })
                }
                "ack_perms" => {
                    let state = app.state::<AppState>();
                    match state.db.get_settings() {
                        Ok(mut s) => {
                            s.permissions_explained = true;
                            let _ = state.db.save_settings(&s);
                            refresh_status(&app);
                            json!({ "ok": true, "op": "ack_perms" })
                        }
                        Err(e) => json!({ "ok": false, "error": e.to_string() }),
                    }
                }
                "set_demo" => {
                    let on = cmd
                        .get("demo_mode")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let state = app.state::<AppState>();
                    match state.db.get_settings() {
                        Ok(mut s) => {
                            s.demo_mode = on;
                            let _ = state.db.save_settings(&s);
                            refresh_status(&app);
                            json!({ "ok": true, "op": "set_demo", "demo_mode": on })
                        }
                        Err(e) => json!({ "ok": false, "error": e.to_string() }),
                    }
                }
                _ => json!({ "ok": false, "error": format!("unknown op: {op}") }),
            };
            debug_loop::write_command_result(&result);
        }
    });
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            debug_loop::init();
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
                        let _ = start_capture_opts(app, false, "tray");
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
            place_overlay(&app.handle());
            refresh_status(&app.handle());
            spawn_command_poller(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_tasks,
            open_provider_console,
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
