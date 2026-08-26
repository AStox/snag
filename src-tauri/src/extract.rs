use base64::Engine;
use serde_json::{json, Value};

use crate::error::{Result, SnagError};
use crate::models::{fixtures, AppSettings, CaptureBundle, ExtractedTask, Provider};

const SYSTEM: &str = r#"You turn an on-screen moment into a single TODO.

Return JSON only:
{"title": string, "notes": string, "due_hint": string|null, "source_app": string|null, "confidence": number}

Rules:
- title is imperative, specific, at most ~80 characters. Something a person would write on a sticky note.
- If the user said something generic ("add this", "add this as a task for me", "snag this"), the SCREEN is the content. Infer the task from what is under/near the cursor (marked with a red ring and dot).
- If the user said something specific, prefer their words as the title; use the screen as notes and context.
- notes capture people, quotes, links, and extra detail visible or spoken. Do not invent.
- due_hint is a short phrase like "Friday" or "Sept 12" when visible or spoken, else null.
- source_app is the app if you can tell, else the provided frontmost app.
- confidence is 0-1.
"#;

fn is_generic(transcript: &str) -> bool {
    let s = transcript
        .trim()
        .to_lowercase()
        .trim_end_matches(['.', '?', '!'])
        .to_string();
    if s.is_empty() {
        return true;
    }
    const PHRASES: &[&str] = &[
        "add this as a task for me",
        "add this as a task",
        "add this",
        "snag this",
        "save this",
        "capture this",
        "remember this",
        "make this a task",
        "todo this",
        "remind me about this",
        "turn this into a task",
    ];
    PHRASES.iter().any(|p| s == *p || s.contains(p))
}

fn clean_title(raw: &str) -> String {
    let re = regex_lite(raw);
    if re.is_empty() {
        raw.trim().to_string()
    } else {
        let mut c = re.chars();
        match c.next() {
            None => re,
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        }
    }
}

fn regex_lite(raw: &str) -> String {
    let mut t = raw.trim().to_string();
    let prefixes = [
        "please add this as a task for me",
        "add this as a task for me",
        "please add this as a task",
        "add this as a task",
        "please add",
        "add this:",
        "add this -",
        "remember to ",
        "remind me to ",
        "create a task to ",
        "create a task ",
    ];
    let lower = t.to_lowercase();
    for p in prefixes {
        if lower.starts_with(p) {
            t = t[p.len()..].trim().trim_start_matches(':').trim().to_string();
            break;
        }
    }
    t.trim_end_matches('.').to_string()
}

pub fn heuristic(transcript: &str, capture: &CaptureBundle) -> ExtractedTask {
    let fixture = capture
        .fixture_id
        .as_deref()
        .and_then(|id| fixtures().into_iter().find(|f| f.id == id));
    if is_generic(transcript) {
        if let Some(f) = fixture {
            return ExtractedTask {
                title: f.caption.into(),
                notes: f.notes_hint.into(),
                due_hint: f.due_hint.map(|s| s.to_string()),
                source_app: Some(f.source_app.into()),
                confidence: 0.62,
            };
        }
        let app = capture
            .source_app
            .clone()
            .unwrap_or_else(|| "the current app".into());
        return ExtractedTask {
            title: format!("Follow up in {app}"),
            notes: if transcript.trim().is_empty() {
                capture.window_title.clone().unwrap_or_default()
            } else {
                format!("Voice: {}", transcript.trim())
            },
            due_hint: None,
            source_app: capture.source_app.clone(),
            confidence: 0.4,
        };
    }
    ExtractedTask {
        title: clean_title(transcript),
        notes: match (&fixture, &capture.source_app) {
            (Some(f), _) => format!("On screen ({}): {}", f.source_app, f.notes_hint),
            (None, Some(app)) => format!("Captured from {app}"),
            _ => String::new(),
        },
        due_hint: fixture.and_then(|f| f.due_hint.map(|s| s.to_string())),
        source_app: capture.source_app.clone(),
        confidence: 0.74,
    }
}

fn parse_extracted(raw: &str) -> Result<ExtractedTask> {
    let trimmed = raw.trim();
    let json_str = if let Some(start) = trimmed.find('{') {
        let end = trimmed.rfind('}').unwrap_or(trimmed.len() - 1);
        &trimmed[start..=end]
    } else {
        trimmed
    };
    let v: Value = serde_json::from_str(json_str)?;
    Ok(ExtractedTask {
        title: v
            .get("title")
            .and_then(|x| x.as_str())
            .unwrap_or("Untitled")
            .trim()
            .to_string(),
        notes: v
            .get("notes")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        due_hint: v
            .get("due_hint")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty() && s != "null"),
        source_app: v
            .get("source_app")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty() && s != "null"),
        confidence: v.get("confidence").and_then(|x| x.as_f64()).unwrap_or(0.5) as f32,
    })
}

fn user_text(transcript: &str, capture: &CaptureBundle) -> String {
    format!(
        "Voice transcript:\n{}\n\nFrontmost app: {}\nWindow title: {}\nCursor: {}, {}\nThe crop is centered on the cursor. A red ring marks the pointer.",
        if transcript.trim().is_empty() { "(none)" } else { transcript.trim() },
        capture.source_app.as_deref().unwrap_or("(unknown)"),
        capture.window_title.as_deref().unwrap_or("(unknown)"),
        capture.cursor_x.round(),
        capture.cursor_y.round()
    )
}

fn b64(png: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(png)
}

pub async fn transcribe_openai(api_key: &str, wav: &[u8]) -> Result<String> {
    if wav.len() < 64 {
        return Ok(String::new());
    }
    let part = reqwest::multipart::Part::bytes(wav.to_vec())
        .file_name("snag.wav")
        .mime_str("audio/wav")
        .map_err(|e| SnagError::from(e.to_string()))?;
    let form = reqwest::multipart::Form::new()
        .text("model", "whisper-1")
        .part("file", part);
    let client = reqwest::Client::new();
    let res = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await?;
    let status = res.status();
    let body = res.text().await?;
    if !status.is_success() {
        return Err(SnagError::from(format!("whisper {status}: {body}")));
    }
    let v: Value = serde_json::from_str(&body)?;
    Ok(v.get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string())
}

async fn openai_vision(settings: &AppSettings, capture: &CaptureBundle, transcript: &str) -> Result<ExtractedTask> {
    let mut content = vec![json!({"type": "text", "text": user_text(transcript, capture)})];
    content.push(json!({
        "type": "image_url",
        "image_url": { "url": format!("data:image/png;base64,{}", b64(&capture.crop_png)) }
    }));
    if settings.send_full_screenshot {
        content.push(json!({
            "type": "image_url",
            "image_url": { "url": format!("data:image/png;base64,{}", b64(&capture.full_png)) }
        }));
    }
    let body = json!({
        "model": settings.model,
        "response_format": { "type": "json_object" },
        "messages": [
            { "role": "system", "content": SYSTEM },
            { "role": "user", "content": content }
        ]
    });
    let client = reqwest::Client::new();
    let res = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(&settings.api_key)
        .json(&body)
        .send()
        .await?;
    let status = res.status();
    let text = res.text().await?;
    if !status.is_success() {
        return Err(SnagError::from(format!("openai {status}: {text}")));
    }
    let v: Value = serde_json::from_str(&text)?;
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| SnagError::from("openai response missing content"))?;
    parse_extracted(content)
}

async fn anthropic_vision(settings: &AppSettings, capture: &CaptureBundle, transcript: &str) -> Result<ExtractedTask> {
    let mut content = Vec::new();
    content.push(json!({
        "type": "image",
        "source": { "type": "base64", "media_type": "image/png", "data": b64(&capture.crop_png) }
    }));
    if settings.send_full_screenshot {
        content.push(json!({
            "type": "image",
            "source": { "type": "base64", "media_type": "image/png", "data": b64(&capture.full_png) }
        }));
    }
    content.push(json!({ "type": "text", "text": format!("{SYSTEM}\n\n{}", user_text(transcript, capture)) }));
    let body = json!({
        "model": settings.model,
        "max_tokens": 800,
        "messages": [{ "role": "user", "content": content }]
    });
    let client = reqwest::Client::new();
    let res = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &settings.api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await?;
    let status = res.status();
    let text = res.text().await?;
    if !status.is_success() {
        return Err(SnagError::from(format!("anthropic {status}: {text}")));
    }
    let v: Value = serde_json::from_str(&text)?;
    let mut buf = String::new();
    if let Some(arr) = v.get("content").and_then(|c| c.as_array()) {
        for part in arr {
            if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                    buf.push_str(t);
                }
            }
        }
    }
    parse_extracted(&buf)
}

pub async fn extract(
    settings: &AppSettings,
    capture: &CaptureBundle,
    transcript: &str,
) -> ExtractedTask {
    if settings.api_key.trim().is_empty() {
        return heuristic(transcript, capture);
    }
    let result = match settings.provider {
        Provider::Openai => openai_vision(settings, capture, transcript).await,
        Provider::Anthropic => anthropic_vision(settings, capture, transcript).await,
    };
    match result {
        Ok(mut task) => {
            if task.title.trim().is_empty() {
                return heuristic(transcript, capture);
            }
            if task.source_app.is_none() {
                task.source_app = capture.source_app.clone();
            }
            task
        }
        Err(err) => {
            log::warn!("vision extract failed: {err}");
            let mut h = heuristic(transcript, capture);
            if !h.notes.is_empty() {
                h.notes = format!("{}\n\n(model unavailable: {err})", h.notes);
            } else {
                h.notes = format!("model unavailable: {err}");
            }
            h
        }
    }
}
