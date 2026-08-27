use base64::Engine;
use serde_json::{json, Value};

use crate::error::{Result, SnagError};
use crate::models::{fixtures, AppSettings, CaptureBundle, ExtractedTask, FixtureMeta, Provider};

const DOCUMENT_CHAR_LIMIT: usize = 24_000;

const SYSTEM: &str = r#"You extract todos from an on-screen moment. The user pointed at this with a hotkey. That does not mean there is work.

Return JSON only:
{"tasks":[{"title": string, "notes": string, "due_hint": string|null, "source_app": string|null, "confidence": number}]}

Rules:
- If it looks like a meeting transcript (Grain, Zoom recap, Fathom, etc.), treat it as a transcript: pull every action item, owner, due date. Multiple tasks are expected.
- If it's a single Slack ask, one task is fine.
- Empty tasks array is the correct answer when there is nothing a person would put on a sticky note. Prefer {"tasks":[]} over inventing work. The overlay will say "Nothing to snag".
- Ignore chit-chat, reactions, jokes, tweets/posts with no ask, "looks good", "lol", "they both look good", likes, and UI chrome. Do not file the entire post as a task.
- Do not invent a junk todo such as "Follow up in Grain" or "Follow up in Slack" when nothing specific is actionable.
- Only file a task if a reasonable person would do it later: an ask, a commitment, an action item, a bug, a PR to review, a date they own.
- On-screen DOCUMENT TEXT is the source of truth when present. Images are layout/context (who is highlighted, UI chrome, which app). A downscaled full display may be attached for layout; the crop is what they pointed at.
- title is imperative, specific, at most ~80 characters. Something a person would write on a sticky note. Include owner when visible.
- notes capture people, quotes, links, extra detail, and due context. Do not invent.
- due_hint is a short phrase like "Friday" or "Sept 12" when visible, else null.
- source_app is the app if you can tell, else the provided frontmost app.
- confidence is 0-1.
"#;

const ACTION_MARKERS: &[&str] = &[
    "todo",
    "action:",
    "action item",
    "i'll",
    "i will",
    "can you",
    "follow up",
    "we should",
    "please ",
    "need to",
    "needs to",
    "assigned to",
    "[ ]",
    "- [ ]",
    "will you",
    "let's",
    "make sure",
    "owner:",
];

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
    t = t
        .trim_start_matches(['-', '*', '•', '–', '—'])
        .trim()
        .to_string();
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
        "todo:",
        "todo ",
        "action item:",
        "action:",
        "action -",
        "[ ] ",
        "- [ ] ",
    ];
    let lower = t.to_lowercase();
    for p in prefixes {
        if lower.starts_with(p) {
            t = t[p.len()..]
                .trim()
                .trim_start_matches(':')
                .trim()
                .to_string();
            break;
        }
    }
    t.trim_end_matches('.').to_string()
}

pub fn looks_like_action(line: &str) -> bool {
    let l = line.trim().to_lowercase();
    if l.len() < 8 {
        return false;
    }
    if (l.contains("github.com/") || l.contains("gitlab.com/"))
        && (l.contains("/pull/") || l.contains("/issues/") || l.contains("/merge_requests/"))
    {
        return true;
    }
    ACTION_MARKERS.iter().any(|m| l.contains(m))
}

fn is_long_document(doc: &str) -> bool {
    doc.chars().count() > 400 || doc.lines().filter(|l| !l.trim().is_empty()).count() > 6
}

fn truncate_chars(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let idx = s
        .char_indices()
        .nth(max)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    format!("{}…", &s[..idx])
}

fn document_for_model(capture: &CaptureBundle) -> String {
    match &capture.document_text {
        Some(s) if !s.trim().is_empty() => truncate_chars(s.trim(), DOCUMENT_CHAR_LIMIT),
        _ => String::new(),
    }
}

fn make_task(
    title: String,
    notes: String,
    due_hint: Option<String>,
    source_app: Option<String>,
    confidence: f32,
) -> ExtractedTask {
    ExtractedTask {
        title,
        notes,
        due_hint,
        source_app,
        confidence,
        has_task: true,
    }
}

fn fixture_tasks(f: &FixtureMeta) -> Vec<ExtractedTask> {
    let mut out = vec![make_task(
        f.caption.into(),
        f.notes_hint.into(),
        f.due_hint.map(|s| s.to_string()),
        Some(f.source_app.into()),
        0.62,
    )];
    if let Some(extra) = f.extra_caption {
        out.push(make_task(
            extra.into(),
            f.notes_hint.into(),
            f.due_hint.map(|s| s.to_string()),
            Some(f.source_app.into()),
            0.55,
        ));
    }
    out
}

fn split_action_lines(doc: &str, capture: &CaptureBundle) -> Vec<ExtractedTask> {
    let mut tasks = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in doc.lines() {
        let line = line.trim();
        if line.len() < 8 || !looks_like_action(line) {
            continue;
        }
        let title = clean_title(line);
        if !should_file_title(&title) {
            continue;
        }
        let key = title.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        tasks.push(make_task(
            title,
            format!("From on-screen document:\n{}", truncate_chars(line, 400)),
            None,
            capture.source_app.clone(),
            0.55,
        ));
        if tasks.len() >= 24 {
            break;
        }
    }
    tasks
}

pub fn heuristic(transcript: &str, capture: &CaptureBundle) -> Vec<ExtractedTask> {
    let fixture = capture
        .fixture_id
        .as_deref()
        .and_then(|id| fixtures().into_iter().find(|f| f.id == id));

    if let Some(doc) = capture.document_text.as_deref() {
        let trimmed = doc.trim();
        if !trimmed.is_empty() {
            if is_long_document(trimmed) {
                let split = split_action_lines(trimmed, capture);
                if !split.is_empty() {
                    return split;
                }
                // Long doc with no action-like lines: do not invent work (never "Follow up in Grain").
                if fixture.is_none() {
                    return Vec::new();
                }
            } else if looks_like_action(trimmed) || trimmed.lines().any(looks_like_action) {
                let split = split_action_lines(trimmed, capture);
                if !split.is_empty() {
                    return split;
                }
                let title = clean_title(trimmed.lines().next().unwrap_or(trimmed));
                if should_file_title(&title) {
                    return vec![make_task(
                        title,
                        truncate_chars(trimmed, 800),
                        None,
                        capture.source_app.clone(),
                        0.58,
                    )];
                }
            }
        }
    }

    if let Some(f) = fixture {
        return fixture_tasks(&f);
    }

    if !is_generic(transcript) {
        return vec![make_task(
            clean_title(transcript),
            match &capture.source_app {
                Some(app) => format!("Captured from {app}"),
                None => String::new(),
            },
            None,
            capture.source_app.clone(),
            0.74,
        )];
    }

    Vec::new()
}

fn parse_one(v: &Value) -> Option<ExtractedTask> {
    let title = v
        .get("title")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if title.is_empty() {
        return None;
    }
    let has_task = v
        .get("has_task")
        .or_else(|| v.get("hasTask"))
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    if !has_task {
        return None;
    }
    Some(ExtractedTask {
        title,
        notes: v
            .get("notes")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        due_hint: v
            .get("due_hint")
            .or_else(|| v.get("dueHint"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty() && s != "null"),
        source_app: v
            .get("source_app")
            .or_else(|| v.get("sourceApp"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty() && s != "null"),
        confidence: v.get("confidence").and_then(|x| x.as_f64()).unwrap_or(0.5) as f32,
        has_task: true,
    })
}

fn json_blob(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let Some(start) = trimmed.find('{') {
        let end = trimmed.rfind('}').unwrap_or(trimmed.len() - 1);
        &trimmed[start..=end]
    } else {
        trimmed
    }
}

fn parse_tasks(raw: &str) -> Result<Vec<ExtractedTask>> {
    let v: Value = serde_json::from_str(json_blob(raw))?;
    if let Some(arr) = v.get("tasks").and_then(|x| x.as_array()) {
        return Ok(arr.iter().filter_map(parse_one).collect());
    }
    // Legacy single-object shape.
    Ok(parse_one(&v).into_iter().collect())
}

fn user_text(transcript: &str, capture: &CaptureBundle) -> String {
    let doc = document_for_model(capture);
    let doc_block = if doc.is_empty() {
        "(none — infer from the images; do not invent work)".to_string()
    } else {
        doc
    };
    format!(
        "On-screen document text (source of truth when present — may be a Grain/Zoom/Fathom transcript, Slack thread, PR, etc.):\n{}\n\nSpoken transcript (usually empty):\n{}\n\nFrontmost app: {}\nWindow title: {}\nCursor: {}, {}\nThe crop is centered on the cursor. A red ring marks the pointer. Images are layout/context. Extract every action item as a separate task. If nothing is actionable, return {{\"tasks\":[]}}.",
        doc_block,
        if transcript.trim().is_empty() {
            "(none)"
        } else {
            transcript.trim()
        },
        capture.source_app.as_deref().unwrap_or("(unknown)"),
        capture.window_title.as_deref().unwrap_or("(unknown)"),
        capture.cursor_x.round(),
        capture.cursor_y.round()
    )
}

fn b64(png: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(png)
}

#[allow(dead_code)]
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExtractReport {
    pub tasks: Vec<ExtractedTask>,
    pub path: String,
    pub model_raw: Option<String>,
    pub error: Option<String>,
    pub document_chars: usize,
    pub used_images: bool,
    pub task_count: usize,
}

impl ExtractReport {
    fn wrap(
        mut tasks: Vec<ExtractedTask>,
        capture: &CaptureBundle,
        path: &str,
        raw: Option<String>,
        error: Option<String>,
        used_images: bool,
    ) -> Self {
        tasks = polish(tasks, capture);
        let task_count = tasks.len();
        Self {
            tasks,
            path: path.into(),
            model_raw: raw,
            error,
            document_chars: capture
                .document_text
                .as_deref()
                .map(|s| s.chars().count())
                .unwrap_or(0),
            used_images,
            task_count,
        }
    }
}

async fn openai_compatible_chat(
    settings: &AppSettings,
    capture: &CaptureBundle,
    transcript: &str,
    with_images: bool,
    url: &str,
    label: &str,
) -> Result<(Vec<ExtractedTask>, String)> {
    let mut content = vec![json!({"type": "text", "text": user_text(transcript, capture)})];
    if with_images {
        content.push(json!({
            "type": "image_url",
            "image_url": { "url": format!("data:image/png;base64,{}", b64(&capture.crop_png)) }
        }));
        if settings.send_full_screenshot && !capture.full_png.is_empty() {
            content.push(json!({
                "type": "image_url",
                "image_url": { "url": format!("data:image/png;base64,{}", b64(&capture.full_png)) }
            }));
        }
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
        .post(url)
        .bearer_auth(&settings.api_key)
        .json(&body)
        .send()
        .await?;
    let status = res.status();
    let text = res.text().await?;
    if !status.is_success() {
        return Err(SnagError::from(format!("{label} {status}: {text}")));
    }
    let v: Value = serde_json::from_str(&text)?;
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| SnagError::from(format!("{label} response missing content")))?;
    Ok((parse_tasks(content)?, content.to_string()))
}

async fn openai_chat(
    settings: &AppSettings,
    capture: &CaptureBundle,
    transcript: &str,
    with_images: bool,
) -> Result<(Vec<ExtractedTask>, String)> {
    openai_compatible_chat(
        settings,
        capture,
        transcript,
        with_images,
        "https://api.openai.com/v1/chat/completions",
        "openai",
    )
    .await
}

async fn xai_chat(
    settings: &AppSettings,
    capture: &CaptureBundle,
    transcript: &str,
    with_images: bool,
) -> Result<(Vec<ExtractedTask>, String)> {
    openai_compatible_chat(
        settings,
        capture,
        transcript,
        with_images,
        "https://api.x.ai/v1/chat/completions",
        "xai",
    )
    .await
}

async fn anthropic_chat(
    settings: &AppSettings,
    capture: &CaptureBundle,
    transcript: &str,
    with_images: bool,
) -> Result<(Vec<ExtractedTask>, String)> {
    let mut content = Vec::new();
    // Lead with text (source of truth), then images for layout/context.
    content.push(json!({ "type": "text", "text": format!("{SYSTEM}\n\n{}", user_text(transcript, capture)) }));
    if with_images {
        content.push(json!({
            "type": "image",
            "source": { "type": "base64", "media_type": "image/png", "data": b64(&capture.crop_png) }
        }));
        if settings.send_full_screenshot && !capture.full_png.is_empty() {
            content.push(json!({
                "type": "image",
                "source": { "type": "base64", "media_type": "image/png", "data": b64(&capture.full_png) }
            }));
        }
    }
    let body = json!({
        "model": settings.model,
        "max_tokens": 4096,
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
    Ok((parse_tasks(&buf)?, buf))
}

async fn provider_chat(
    settings: &AppSettings,
    capture: &CaptureBundle,
    transcript: &str,
    with_images: bool,
) -> Result<(Vec<ExtractedTask>, String)> {
    match settings.provider {
        Provider::Openai => openai_chat(settings, capture, transcript, with_images).await,
        Provider::Anthropic => anthropic_chat(settings, capture, transcript, with_images).await,
        Provider::Xai => xai_chat(settings, capture, transcript, with_images).await,
    }
}

fn polish(mut tasks: Vec<ExtractedTask>, capture: &CaptureBundle) -> Vec<ExtractedTask> {
    for t in &mut tasks {
        if t.source_app.is_none() {
            t.source_app = capture.source_app.clone();
        }
        t.has_task = !t.title.trim().is_empty();
    }
    tasks.retain(should_file);
    tasks
}

pub async fn extract(
    settings: &AppSettings,
    capture: &CaptureBundle,
    transcript: &str,
) -> ExtractReport {
    if settings.api_key.trim().is_empty() {
        return ExtractReport::wrap(
            heuristic(transcript, capture),
            capture,
            "heuristic",
            None,
            None,
            false,
        );
    }
    let has_doc = capture
        .document_text
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    match provider_chat(settings, capture, transcript, true).await {
        Ok((tasks, raw)) => ExtractReport::wrap(tasks, capture, "vision+text", Some(raw), None, true),
        Err(err) => {
            log::warn!("vision+text extract failed: {err}");
            if has_doc {
                match provider_chat(settings, capture, transcript, false).await {
                    Ok((tasks, raw)) => {
                        return ExtractReport::wrap(
                            tasks,
                            capture,
                            "text-only",
                            Some(raw),
                            Some(err.to_string()),
                            false,
                        );
                    }
                    Err(err2) => {
                        log::warn!("text-only extract failed: {err2}");
                        let mut h = heuristic(transcript, capture);
                        if let Some(first) = h.first_mut() {
                            if !first.notes.is_empty() {
                                first.notes = format!("{}

(model unavailable: {err2})", first.notes);
                            } else {
                                first.notes = format!("model unavailable: {err2}");
                            }
                        }
                        return ExtractReport::wrap(
                            h,
                            capture,
                            "heuristic-fallback",
                            None,
                            Some(err2.to_string()),
                            false,
                        );
                    }
                }
            }
            let mut h = heuristic(transcript, capture);
            if let Some(first) = h.first_mut() {
                if !first.notes.is_empty() {
                    first.notes = format!("{}

(model unavailable: {err})", first.notes);
                } else {
                    first.notes = format!("model unavailable: {err}");
                }
            }
            ExtractReport::wrap(
                h,
                capture,
                "heuristic-fallback",
                None,
                Some(err.to_string()),
                true,
            )
        }
    }
}

fn is_chatter(title: &str) -> bool {
    let s = title
        .trim()
        .to_lowercase()
        .trim_end_matches(['.', '!', '?', ',', '~'])
        .trim()
        .to_string();
    if s.is_empty() {
        return true;
    }
    const EXACT: &[&str] = &[
        "lgtm",
        "lol",
        "lmao",
        "haha",
        "thanks",
        "thank you",
        "nice",
        "cool",
        "yeah",
        "yep",
        "yup",
        "ok",
        "okay",
        "same",
        "agreed",
        "true",
        "facts",
        "mood",
        "this",
        "that",
        "idk",
        "wow",
        "omg",
        "bruh",
        "sounds good",
        "looks good",
        "looks great",
        "looks fine",
        "love this",
        "this is fire",
        "they both look good",
        "both look good",
    ];
    if EXACT.iter().any(|p| s == *p) {
        return true;
    }
    s.contains("look good") || s.contains("looks good") || s.contains("looks great")
}

pub fn should_file_title(title: &str) -> bool {
    let t = title.trim();
    if t.is_empty() {
        return false;
    }
    // Whole tweets / dumped AX blobs are not sticky notes.
    if t.chars().count() > 140 {
        return false;
    }
    let lower = t.to_lowercase();
    if lower == "untitled" || lower == "nothing to snag" || lower == "n/a" || lower == "none" {
        return false;
    }
    if is_chatter(t) {
        return false;
    }
    if lower.starts_with("follow up in ") {
        let rest = lower["follow up in ".len()..].trim();
        // Junk like "Follow up in Grain" / "Follow up in Slack" — app name only.
        if !rest.is_empty() && !rest.contains(' ') && rest.len() < 24 {
            return false;
        }
    }
    true
}

pub fn should_file(task: &ExtractedTask) -> bool {
    task.has_task && should_file_title(&task.title)
}

pub fn overlay_title(tasks: &[ExtractedTask]) -> String {
    match tasks.len() {
        0 => "Nothing to snag".into(),
        1 => tasks[0].title.clone(),
        n => format!("Snagged {n} tasks"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junk_follow_up_in_grain() {
        assert!(!should_file_title("Follow up in Grain"));
        assert!(!should_file_title("follow up in slack"));
        assert!(should_file_title("Follow up with Adam about the Q3 launch"));
        assert!(!should_file_title("they both look good!"));
        assert!(!should_file_title("Looks good"));
        assert!(!should_file_title("a".repeat(141).as_str()));
    }

    #[test]
    fn chatter_and_tweets_are_nothing_to_snag() {
        let tweet = "lets be honest this is the funniest thing I have seen all week https://x.com/foo/status/1";
        let tasks = heuristic("", &bundle(Some(tweet), None));
        assert!(tasks.is_empty(), "{:?}", tasks.iter().map(|t| t.title.clone()).collect::<Vec<_>>());
        let chat = "Maya: they both look good!\nAdam: lol yeah";
        let tasks = heuristic("", &bundle(Some(chat), None));
        assert!(tasks.is_empty(), "{:?}", tasks.iter().map(|t| t.title.clone()).collect::<Vec<_>>());
        let ask = "Can you review PR 462 today?";
        let tasks = heuristic("", &bundle(Some(ask), None));
        assert_eq!(tasks.len(), 1, "{:?}", tasks.iter().map(|t| t.title.clone()).collect::<Vec<_>>());
    }

    #[test]
    fn action_markers() {
        assert!(looks_like_action("TODO: ship the crop"));
        assert!(looks_like_action("I'll send the recap by Friday"));
        assert!(looks_like_action("Can you review PR 482?"));
        assert!(looks_like_action("We should ping legal"));
        assert!(looks_like_action("Action: Maya owns the docs"));
        assert!(!looks_like_action("hi"));
        assert!(!looks_like_action("The weather is nice today"));
        assert!(!looks_like_action("lets be honest this is funny"));
        assert!(looks_like_action("Let's ping legal this week"));
    }

    #[test]
    fn parse_tasks_array() {
        let raw = r#"{"tasks":[{"title":"Ping Maya","notes":"PR 482","due_hint":null,"source_app":"GitHub","confidence":0.8},{"title":"","notes":"skip"}]}"#;
        let tasks = parse_tasks(raw).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Ping Maya");
    }

    #[test]
    fn parse_empty_and_legacy() {
        assert!(parse_tasks(r#"{"tasks":[]}"#).unwrap().is_empty());
        let one = parse_tasks(r#"{"title":"One thing","notes":"","has_task":true,"confidence":0.4}"#).unwrap();
        assert_eq!(one.len(), 1);
        let none = parse_tasks(r#"{"title":"","has_task":false}"#).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn overlay_copy() {
        let t = make_task("Hello".into(), "".into(), None, None, 0.5);
        assert_eq!(overlay_title(&[]), "Nothing to snag");
        assert_eq!(overlay_title(&[t.clone()]), "Hello");
        assert_eq!(overlay_title(&[t.clone(), t]), "Snagged 2 tasks");
    }

    fn bundle(doc: Option<&str>, fixture: Option<&str>) -> CaptureBundle {
        CaptureBundle {
            full_png: vec![],
            crop_png: vec![],
            cursor_x: 0.0,
            cursor_y: 0.0,
            source_app: Some("Grain".into()),
            window_title: Some("Weekly recap".into()),
            fixture_id: fixture.map(|s| s.to_string()),
            document_text: doc.map(|s| s.to_string()),
        }
    }

    #[test]
    fn long_transcript_splits_actions() {
        let doc = "Weekly recap\n\nMaya: we should ship the crop this week.\nAdam: I will send the timeline by Friday.\nSam: can you review the legal copy?\n(small talk about lunch)\nTODO: ping design for icons\n";
        let tasks = heuristic("", &bundle(Some(doc), None));
        let titles: Vec<_> = tasks.iter().map(|t| t.title.to_lowercase()).collect();
        assert!(titles.len() >= 3, "{titles:?}");
        assert!(titles.iter().any(|t| t.contains("crop") || t.contains("ship")));
        assert!(titles.iter().any(|t| t.contains("timeline") || t.contains("friday") || t.contains("send")));
        assert!(!titles.iter().any(|t| t == "follow up in grain"));
    }

    #[test]
    fn long_doc_without_actions_is_empty() {
        let doc = "lorem ipsum ".repeat(80);
        let tasks = heuristic("", &bundle(Some(&doc), None));
        assert!(tasks.is_empty());
    }

    #[test]
    fn fixture_returns_caption_and_extra() {
        let tasks = heuristic("", &bundle(None, Some("slack-thread")));
        assert_eq!(tasks.len(), 2);
        assert!(tasks[0].title.contains("Adam"));
        assert!(tasks[1].title.to_lowercase().contains("legal"));
    }
}
