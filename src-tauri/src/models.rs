use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Inbox,
    Doing,
    Done,
}

impl Default for TaskStatus {
    fn default() -> Self {
        Self::Inbox
    }
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Doing => "doing",
            Self::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "doing" => Self::Doing,
            "done" => Self::Done,
            _ => Self::Inbox,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub title: String,
    pub notes: String,
    pub status: TaskStatus,
    pub due_hint: Option<String>,
    pub source_app: Option<String>,
    pub source_window: Option<String>,
    pub confidence: Option<f32>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPatch {
    pub title: Option<String>,
    pub notes: Option<String>,
    pub status: Option<TaskStatus>,
    pub due_hint: Option<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Openai,
    Anthropic,
}

impl Default for Provider {
    fn default() -> Self {
        Self::Openai
    }
}

impl Provider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Anthropic => "anthropic",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "anthropic" => Self::Anthropic,
            _ => Self::Openai,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub hotkey: String,
    pub provider: Provider,
    pub api_key: String,
    pub model: String,
    pub send_full_screenshot: bool,
    pub demo_mode: bool,
    pub demo_fixture: String,
    pub permissions_explained: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hotkey: "alt+space".into(),
            provider: Provider::Openai,
            api_key: String::new(),
            model: "gpt-4o".into(),
            send_full_screenshot: true,
            demo_mode: !cfg!(target_os = "macos"),
            demo_fixture: "auto".into(),
            permissions_explained: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    pub phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SessionState {
    pub fn idle() -> Self {
        Self {
            phase: "idle".into(),
            hint: None,
            title: None,
            error: None,
        }
    }
    pub fn listening() -> Self {
        Self {
            phase: "listening".into(),
            hint: Some("Point at something. Speak, pause, or hit the hotkey again.".into()),
            title: None,
            error: None,
        }
    }
    pub fn processing() -> Self {
        Self {
            phase: "processing".into(),
            hint: Some("Figuring it out…".into()),
            title: None,
            error: None,
        }
    }
    pub fn done(title: String) -> Self {
        Self {
            phase: "done".into(),
            hint: Some("Saved locally — screenshot and audio discarded.".into()),
            title: Some(title),
            error: None,
        }
    }
    pub fn explain() -> Self {
        Self {
            phase: "explain".into(),
            hint: Some("Screen Recording and Microphone. Continues after you allow.".into()),
            title: None,
            error: None,
        }
    }
    pub fn error(msg: impl Into<String>) -> Self {
        let m = msg.into();
        Self {
            phase: "error".into(),
            hint: Some(m.clone()),
            title: None,
            error: Some(m),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    pub screen: String,
    pub microphone: String,
    pub platform: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedTask {
    pub title: String,
    pub notes: String,
    pub due_hint: Option<String>,
    pub source_app: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub struct CaptureBundle {
    pub full_png: Vec<u8>,
    pub crop_png: Vec<u8>,
    pub cursor_x: f64,
    pub cursor_y: f64,
    pub source_app: Option<String>,
    pub window_title: Option<String>,
    pub fixture_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FixtureMeta {
    pub id: &'static str,
    pub source_app: &'static str,
    pub window_title: &'static str,
    pub caption: &'static str,
    pub notes_hint: &'static str,
    pub due_hint: Option<&'static str>,
    pub cursor_x: u32,
    pub cursor_y: u32,
    pub png: &'static [u8],
}

pub fn fixtures() -> [FixtureMeta; 2] {
    [
        FixtureMeta {
            id: "slack-thread",
            source_app: "Slack",
            window_title: "engineering — Q3 launch",
            caption: "Follow up with Adam about the Q3 launch timeline",
            notes_hint: "Adam asked whether the mobile cut is still targeting Sept 12, and whether legal review is blocking the help-center copy. Thread in #eng-launch.",
            due_hint: Some("Sept 12"),
            cursor_x: 560,
            cursor_y: 318,
            png: include_bytes!("../fixtures/slack-thread.png"),
        },
        FixtureMeta {
            id: "github-pr",
            source_app: "GitHub",
            window_title: "PR #482 · cursor-aware screenshot crop",
            caption: "Review PR #482: add cursor-aware screenshot crop",
            notes_hint: "Maya requested review on feat/snag-crop. Adds a 900px-radius crop around the cursor and draws a marker on the full display capture. Waiting on a look at capture.rs.",
            due_hint: None,
            cursor_x: 640,
            cursor_y: 280,
            png: include_bytes!("../fixtures/github-pr.png"),
        },
    ]
}
