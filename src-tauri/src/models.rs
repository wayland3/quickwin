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

/// 剪贴板列表筛选（与 UI 的 chip 一一对应）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ClipFilter {
    #[default]
    All,
    Text,
    Image,
    Files,
    Favorites,
}

/// 激活方式：粘贴（Enter）或仅复制（Ctrl+Enter）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PasteMode {
    Paste,
    Copy,
}

/// `clipboard_list` 返回结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardListResult {
    pub items: Vec<ClipboardItem>,
    pub total: i64,
}

/// `clipboard_get` 返回结构（详情比列表多正文与 blob 绝对路径）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardDetail {
    pub id: i64,
    pub kind: ClipKind,
    pub preview: String,
    pub text_body: Option<String>,
    pub html: Option<String>,
    pub rtf: Option<String>,
    pub file_list: Option<Vec<String>>,
    /// 与 `file_list` 一一对应；文件仍存在为 true（缺失项在预览区标红）
    pub file_exists: Option<Vec<bool>>,
    /// 图片 blob 的绝对路径（前端用 `convertFileSrc` 转换）
    pub blob_path: Option<String>,
    pub thumb_path: Option<String>,
    pub byte_size: i64,
    pub created_at: i64,
    pub last_copied_at: i64,
    pub copy_count: i64,
    pub pinned: bool,
    pub truncated: bool,
    pub source_app: Option<String>,
}
