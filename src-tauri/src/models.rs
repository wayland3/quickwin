use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClipKind {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "html")]
    Html,
    #[serde(rename = "rtf")]
    Rtf,
    #[serde(rename = "image")]
    Image,
    #[serde(rename = "files")]
    Files,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardItem {
    pub id: i64,
    pub kind: ClipKind,
    pub preview: String,
    pub source_app: Option<String>,
    pub created_at: i64,
    pub last_copied_at: i64,
    pub copy_count: i64,
    pub pinned: bool,
    pub sensitive: bool, // 未使用（敏感内容过滤功能已移除）
    pub truncated: bool,
    pub byte_size: i64,
    pub thumb_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LauncherKind {
    #[serde(rename = "app")]
    App,
    #[serde(rename = "file")]
    File,
    #[serde(rename = "command")]
    Command,
    #[serde(rename = "web")]
    Web,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherItem {
    pub key: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub kind: LauncherKind,
    pub icon: Option<String>,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionOffer {
    pub id: String,
    pub group: String,
    pub label: String,
    pub preview: String,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedKind {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionContext {
    pub text: String,
    pub kinds: Vec<DetectedKind>,
    pub source_app: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub launcher_hotkey: String,
    pub clipboard_hotkey: String,
    pub actions_hotkey: String,
    pub retention_days: i32,
    pub max_items: i32,
    pub autostart: bool,
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            launcher_hotkey: "Alt+Space".to_string(),
            clipboard_hotkey: "Alt+V".to_string(),
            actions_hotkey: "Alt+X".to_string(),
            retention_days: 15,
            max_items: 1000,
            autostart: false,
            theme: "system".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
}
