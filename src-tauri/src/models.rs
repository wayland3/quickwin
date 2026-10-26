use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardItem {
    pub id: String,
    pub content_type: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherItem {
    pub id: String,
    pub name: String,
    pub path: String,
    pub frecency: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionOffer {
    pub id: String,
    pub label: String,
    pub action_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub launcher_hotkey: String,
    pub clipboard_hotkey: String,
    pub actions_hotkey: String,
    pub retention_days: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
}
