use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use crate::models::{AppSettings, CaptureBundle, PermissionStatus};

const PORT: u16 = 17333;

static HUB: OnceLock<DebugHub> = OnceLock::new();

#[derive(Clone)]
pub struct DebugHub {
    pub root: PathBuf,
}

pub fn hub() -> Option<&'static DebugHub> {
    HUB.get()
}

pub fn root() -> PathBuf {
    if let Some(h) = HUB.get() {
        return h.root.clone();
    }
    default_root()
}

fn default_root() -> PathBuf {
    // Compile-time repo path during `tauri dev` / local builds.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(repo) = manifest.parent() {
        let p = repo.join(".snag-debug");
        if repo.exists() {
            return p;
        }
    }
    dirs_home()
        .map(|h| h.join("Library/Application Support/com.astox.snag/debug"))
        .unwrap_or_else(|| PathBuf::from("/tmp/snag-debug"))
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub fn init() -> PathBuf {
    let root = default_root();
    let _ = fs::create_dir_all(root.join("sessions"));
    let hub = DebugHub { root: root.clone() };
    let _ = HUB.set(hub);
    event(
        "boot",
        json!({
            "dir": root.display().to_string(),
            "control": format!("http://127.0.0.1:{PORT}"),
        }),
    );
    eprintln!("[snag-debug] dumps at {} — control http://127.0.0.1:{PORT}", root.display());
    let serve_root = root.clone();
    thread::Builder::new()
        .name("snag-debug-http".into())
        .spawn(move || serve(serve_root))
        .ok();
    root
}

pub fn event(kind: &str, data: Value) {
    let rec = json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "kind": kind,
        "data": data,
    });
    eprintln!("[snag-debug] {kind} {data}");
    let path = root().join("events.jsonl");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{rec}");
    }
}

pub fn write_status(value: &Value) {
    write_json(&root().join("status.json"), value);
}

pub fn write_json(path: &Path, value: &Value) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, serde_json::to_vec_pretty(value).unwrap_or_default());
}

pub fn redact_settings(s: &AppSettings) -> Value {
    json!({
        "hotkey": s.hotkey,
        "provider": s.provider.as_str(),
        "model": s.model,
        "sendFullScreenshot": s.send_full_screenshot,
        "demoMode": s.demo_mode,
        "demoFixture": s.demo_fixture,
        "permissionsExplained": s.permissions_explained,
        "hasApiKey": !s.api_key.trim().is_empty(),
        "apiKeyLen": s.api_key.trim().len(),
    })
}

pub struct SessionDump {
    pub id: String,
    pub dir: PathBuf,
}

impl SessionDump {
    pub fn begin(trigger: &str) -> Self {
        let id = format!(
            "{}-{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%S"),
            &uuid::Uuid::new_v4().to_string()[..8]
        );
        let dir = root().join("sessions").join(&id);
        let _ = fs::create_dir_all(&dir);
        write_json(
            &dir.join("meta.json"),
            &json!({
                "id": id,
                "trigger": trigger,
                "startedAt": chrono::Utc::now().to_rfc3339(),
            }),
        );
        event("session_begin", json!({"id": id, "trigger": trigger}));
        Self { id, dir }
    }

    pub fn write_capture(
        &self,
        bundle: &CaptureBundle,
        settings: &AppSettings,
        perms: &PermissionStatus,
    ) {
        let _ = fs::write(self.dir.join("full.png"), &bundle.full_png);
        let _ = fs::write(self.dir.join("crop.png"), &bundle.crop_png);
        if let Some(doc) = bundle.document_text.as_deref() {
            let _ = fs::write(self.dir.join("document.txt"), doc);
        }
        let capture = json!({
            "sourceApp": bundle.source_app,
            "windowTitle": bundle.window_title,
            "fixtureId": bundle.fixture_id,
            "cursorX": bundle.cursor_x,
            "cursorY": bundle.cursor_y,
            "fullPngBytes": bundle.full_png.len(),
            "cropPngBytes": bundle.crop_png.len(),
            "documentChars": bundle.document_text.as_ref().map(|s| s.chars().count()).unwrap_or(0),
            "documentBytes": bundle.document_text.as_ref().map(|s| s.len()).unwrap_or(0),
            "hasDocument": bundle.document_text.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false),
        });
        write_json(&self.dir.join("capture.json"), &capture);
        write_json(
            &self.dir.join("settings.json"),
            &redact_settings(settings),
        );
        write_json(
            &self.dir.join("permissions.json"),
            &json!({
                "screen": perms.screen,
                "accessibility": perms.accessibility,
                "microphone": perms.microphone,
                "platform": perms.platform,
            }),
        );
        event(
            "capture",
            json!({
                "id": self.id,
                "sourceApp": bundle.source_app,
                "windowTitle": bundle.window_title,
                "documentChars": capture["documentChars"],
                "fullPngBytes": bundle.full_png.len(),
                "demoMode": settings.demo_mode,
                "screen": perms.screen,
                "accessibility": perms.accessibility,
            }),
        );
    }

    pub fn write_extract(&self, report: &Value) {
        write_json(&self.dir.join("extract.json"), report);
        event(
            "extract",
            json!({
                "id": self.id,
                "path": report.get("path"),
                "taskCount": report.get("taskCount"),
                "error": report.get("error"),
            }),
        );
    }

    pub fn finish(&self, result: Value) {
        let mut out = result.clone();
        if let Some(obj) = out.as_object_mut() {
            obj.insert("id".into(), json!(self.id));
            obj.insert("finishedAt".into(), json!(chrono::Utc::now().to_rfc3339()));
            obj.insert("dir".into(), json!(self.dir.display().to_string()));
        }
        write_json(&self.dir.join("result.json"), &out);
        write_json(&root().join("last.json"), &out);
        event("session_end", json!({"id": self.id, "result": out}));
    }
}

pub fn take_command() -> Option<Value> {
    let path = root().join("command.json");
    let raw = fs::read_to_string(&path).ok()?;
    let _ = fs::remove_file(&path);
    serde_json::from_str(&raw).ok()
}

pub fn write_command_result(value: &Value) {
    write_json(&root().join("command.result.json"), value);
}

fn serve(root: PathBuf) {
    let listener = match TcpListener::bind(("127.0.0.1", PORT)) {
        Ok(l) => l,
        Err(err) => {
            event("http_bind_fail", json!({"error": err.to_string(), "port": PORT}));
            return;
        }
    };
    event("http_listen", json!({"url": format!("http://127.0.0.1:{PORT}")}));
    listener.set_nonblocking(false).ok();
    for stream in listener.incoming() {
        match stream {
            Ok(mut s) => {
                let root = root.clone();
                thread::spawn(move || {
                    if let Err(err) = handle_client(&root, &mut s) {
                        event("http_err", json!({"error": err}));
                    }
                });
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

fn handle_client(root: &Path, stream: &mut std::net::TcpStream) -> Result<(), String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = vec![0u8; 16_384];
    let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
    if n == 0 {
        return Ok(());
    }
    let head = String::from_utf8_lossy(&buf[..n]);
    let first = head.lines().next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let pathq = parts.next().unwrap_or("/").to_string();
    let (path, query) = pathq.split_once('?').unwrap_or((pathq.as_str(), ""));
    let body_idx = head.find("\r\n\r\n").map(|i| i + 4).unwrap_or(n);
    let mut body = if body_idx < n {
        buf[body_idx..n].to_vec()
    } else {
        Vec::new()
    };
    let clen = head
        .lines()
        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|s| s.trim().parse::<usize>().unwrap_or(0)))
        .unwrap_or(0);
    while body.len() < clen {
        let mut more = vec![0u8; (clen - body.len()).min(4096)];
        let k = stream.read(&mut more).map_err(|e| e.to_string())?;
        if k == 0 {
            break;
        }
        body.extend_from_slice(&more[..k]);
    }
    let (code, ctype, bytes) = route(root, &method, path, query, &body);
    let header = format!(
        "HTTP/1.1 {code}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    stream.write_all(header.as_bytes()).map_err(|e| e.to_string())?;
    stream.write_all(&bytes).map_err(|e| e.to_string())?;
    Ok(())
}

fn route(root: &Path, method: &str, path: &str, query: &str, body: &[u8]) -> (u16, &'static str, Vec<u8>) {
    match (method, path) {
        ("GET", "/health") => json_ok(json!({
            "ok": true,
            "dir": root.display().to_string(),
            "control": format!("http://127.0.0.1:{PORT}"),
        })),
        ("GET", "/status") => file_or_json(root.join("status.json"), json!({"ok": false, "error": "no status yet"})),
        ("GET", "/last") => file_or_json(root.join("last.json"), json!({"ok": false, "error": "no sessions yet"})),
        ("GET", "/log") => file_bytes(root.join("events.jsonl"), "application/x-ndjson"),
        ("GET", "/sessions") => json_ok(list_sessions(root)),
        ("POST", "/capture") => {
            let skip = query.contains("skip_explain=0");
            queue_command(json!({
                "op": "capture",
                "skip_explain": !skip,
            }))
        }
        ("POST", "/perms") => queue_command(json!({"op": "request_perms"})),
        ("POST", "/ack-perms") => queue_command(json!({"op": "ack_perms"})),
        ("POST", "/demo") => {
            let v: Value = serde_json::from_slice(body).unwrap_or(json!({}));
            queue_command(json!({
                "op": "set_demo",
                "demo_mode": v.get("demo_mode").or_else(|| v.get("demoMode")).and_then(|x| x.as_bool()).unwrap_or(false),
            }))
        }
        ("OPTIONS", _) => (204, "text/plain", Vec::new()),
        _ if method == "GET" && path.starts_with("/sessions/") => serve_session(root, &path["/sessions/".len()..]),
        _ => json_code(404, json!({"ok": false, "error": "not found", "path": path})),
    }
}

fn queue_command(cmd: Value) -> (u16, &'static str, Vec<u8>) {
    write_json(&root().join("command.json"), &cmd);
    event("command_queued", cmd.clone());
    json_code(202, json!({"ok": true, "queued": cmd, "poll": "/last"}))
}

fn list_sessions(root: &Path) -> Value {
    let mut ids: Vec<String> = fs::read_dir(root.join("sessions"))
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    ids.sort();
    ids.reverse();
    json!({ "sessions": ids })
}

fn serve_session(root: &Path, rest: &str) -> (u16, &'static str, Vec<u8>) {
    let rest = rest.trim_matches('/');
    if rest.is_empty() {
        return json_ok(list_sessions(root));
    }
    let (id, file) = rest.split_once('/').unwrap_or((rest, "result.json"));
    let path = root.join("sessions").join(id).join(file);
    let ctype = match Path::new(file).extension().and_then(|s| s.to_str()) {
        Some("png") => "image/png",
        Some("txt") => "text/plain; charset=utf-8",
        Some("jsonl") => "application/x-ndjson",
        _ => "application/json",
    };
    file_bytes(path, ctype)
}

fn file_or_json(path: PathBuf, fallback: Value) -> (u16, &'static str, Vec<u8>) {
    if path.exists() {
        file_bytes(path, "application/json")
    } else {
        json_ok(fallback)
    }
}

fn file_bytes(path: PathBuf, ctype: &'static str) -> (u16, &'static str, Vec<u8>) {
    match fs::read(&path) {
        Ok(b) => (200, ctype, b),
        Err(_) => json_code(404, json!({"ok": false, "error": "missing", "path": path.display().to_string()})),
    }
}

fn json_ok(v: Value) -> (u16, &'static str, Vec<u8>) {
    json_code(200, v)
}

fn json_code(code: u16, v: Value) -> (u16, &'static str, Vec<u8>) {
    (
        code,
        "application/json",
        serde_json::to_vec_pretty(&v).unwrap_or_else(|_| b"{}".to_vec()),
    )
}

