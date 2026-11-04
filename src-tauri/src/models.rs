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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LauncherKind {
    #[serde(rename = "app")]
    App,
    #[serde(rename = "file")]
    File,
    #[serde(rename = "command")]
    Command,
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

/// 应用索引重建结果（`app_index_rebuild` 返回）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppIndexRebuildResult {
    /// 写入的应用条数
    pub count: usize,
}

/// 启动器查询结果（`launcher_query` 返回）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherQueryResult {
    /// 最多 12 条，已按得分排序
    pub items: Vec<LauncherItem>,
    /// 可选提示（如前缀补全建议）
    pub hint: Option<String>,
}

/// 设置（存 settings 表，单条 JSON；v2 migration）
///
/// 所有字段带 serde 默认值，旧版本 JSON 缺字段时自动补默认。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    /// 启动器热键
    #[serde(default = "default_launcher_hotkey")]
    pub launcher_hotkey: String,
    /// 剪贴板热键
    #[serde(default = "default_clipboard_hotkey")]
    pub clipboard_hotkey: String,
    /// 保留天数：7 / 15 / 30 / 90，-1 表示永不
    #[serde(default = "default_retention_days")]
    pub retention_days: i32,
    /// 条数上限：1000–50000
    #[serde(default = "default_max_items")]
    pub max_items: i32,
    /// 是否记录文本（含富文本）
    #[serde(default = "default_true")]
    pub record_text: bool,
    /// 是否记录图片
    #[serde(default = "default_true")]
    pub record_image: bool,
    /// 是否记录文件列表
    #[serde(default = "default_true")]
    pub record_files: bool,
    /// 排除应用（进程名列表，通配匹配在 T-207 实现）
    #[serde(default = "default_excluded_apps")]
    pub excluded_apps: Vec<String>,
    /// 主题：system / light / dark
    #[serde(default = "default_theme")]
    pub theme: String,
    /// 开机自启
    #[serde(default)]
    pub autostart: bool,
    /// 暂停记录（持久化；托盘「暂停直到重启」不写该项）
    #[serde(default)]
    pub pause_recording: bool,
    /// 粘贴后还原原剪贴板（T-205 开关，默认关）
    #[serde(default)]
    pub restore_clipboard_after_paste: bool,
}

/// 启动器热键默认值
fn default_launcher_hotkey() -> String {
    "Alt+Space".to_string()
}
/// 剪贴板热键默认值
fn default_clipboard_hotkey() -> String {
    "Alt+V".to_string()
}
/// 保留天数默认 15 天
fn default_retention_days() -> i32 {
    15
}
/// 条数上限默认 10000
fn default_max_items() -> i32 {
    10_000
}
/// 开关默认开
fn default_true() -> bool {
    true
}
/// 默认主题
fn default_theme() -> String {
    "system".to_string()
}
/// 默认空排除列表
fn default_excluded_apps() -> Vec<String> {
    Vec::new()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            launcher_hotkey: default_launcher_hotkey(),
            clipboard_hotkey: default_clipboard_hotkey(),
            retention_days: default_retention_days(),
            max_items: default_max_items(),
            record_text: true,
            record_image: true,
            record_files: true,
            excluded_apps: default_excluded_apps(),
            theme: default_theme(),
            autostart: false,
            pause_recording: false,
            restore_clipboard_after_paste: false,
        }
    }
}

impl Settings {
    /// 合法的保留天数（-1 = 永不）
    pub const VALID_RETENTION_DAYS: [i32; 5] = [7, 15, 30, 90, -1];
    /// 条数上限范围
    pub const MIN_ITEMS: i32 = 1000;
    pub const MAX_ITEMS_LIMIT: i32 = 50000;

    /// 校验全部字段；错误信息为中文，用于 command 层拒绝非法 patch
    pub fn validate(&self) -> Result<(), String> {
        let hotkeys = [
            ("启动器热键", &self.launcher_hotkey),
            ("剪贴板热键", &self.clipboard_hotkey),
        ];
        for (name, accelerator) in hotkeys {
            validate_hotkey(accelerator).map_err(|reason| format!("{}：{}", name, reason))?;
        }

        // 两个热键不能相同，否则必然注册冲突
        if self.launcher_hotkey == self.clipboard_hotkey {
            return Err("两个全局热键不能重复".to_string());
        }

        if !Self::VALID_RETENTION_DAYS.contains(&self.retention_days) {
            return Err("保留天数只支持 7 / 15 / 30 / 90 天或永不".to_string());
        }

        if !(Self::MIN_ITEMS..=Self::MAX_ITEMS_LIMIT).contains(&self.max_items) {
            return Err(format!(
                "条数上限需在 {} 到 {} 之间",
                Self::MIN_ITEMS,
                Self::MAX_ITEMS_LIMIT
            ));
        }

        if !["system", "light", "dark"].contains(&self.theme.as_str()) {
            return Err("主题只支持 system / light / dark".to_string());
        }

        for app_name in &self.excluded_apps {
            let trimmed = app_name.trim();
            if trimmed.is_empty() || trimmed.len() != app_name.len() {
                return Err("排除应用名称不能为空或含首尾空格".to_string());
            }
            if app_name.len() > 260 {
                return Err("排除应用名称过长".to_string());
            }
        }

        Ok(())
    }
}

/// 热键格式校验：必须为「修饰键+主键」形式，每段非空
///
/// 只校验格式，不检测是否被占用（占用属 T-103 运行时行为）。
fn validate_hotkey(accelerator: &str) -> Result<(), String> {
    let trimmed = accelerator.trim();
    if trimmed.is_empty() {
        return Err("不能为空".to_string());
    }

    let parts: Vec<&str> = trimmed.split('+').map(str::trim).collect();
    if parts.iter().any(|part| part.is_empty()) {
        return Err("格式错误，示例：Alt+Space".to_string());
    }
    if parts.len() < 2 {
        return Err("至少需要一个修饰键，示例：Alt+Space".to_string());
    }

    // 最后一段必须是真正的主键，不能还是修饰键
    //
    // 防止把「Ctrl+Super」这种只有修饰键的残缺组合存进数据库：
    // 它注册时必然解析失败，且会让启动流程反复回退默认键。
    let Some(main_key) = parts.last() else {
        return Err("格式错误，示例：Alt+Space".to_string());
    };
    const MODIFIER_NAMES: [&str; 11] = [
        "ctrl", "control", "alt", "option", "shift", "super", "meta", "win", "os", "hyper", "cmd",
    ];
    if MODIFIER_NAMES.contains(&main_key.to_ascii_lowercase().as_str()) {
        return Err("缺少主键，示例：Alt+Space（不能只用修饰键组合）".to_string());
    }

    Ok(())
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

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    #[allow(non_snake_case)]
    fn 默认设置校验通过() {
        assert!(Settings::default().validate().is_ok());
    }

    #[test]
    #[allow(non_snake_case)]
    fn 非法保留天数被拒绝() {
        let first = Settings {
            retention_days: 16,
            ..Settings::default()
        };
        assert!(first.validate().is_err());

        let second = Settings {
            retention_days: -2,
            ..Settings::default()
        };
        assert!(second.validate().is_err());
    }

    #[test]
    #[allow(non_snake_case)]
    fn 条数上限越界被拒绝() {
        let low = Settings {
            max_items: 999,
            ..Settings::default()
        };
        assert!(low.validate().is_err());

        let high = Settings {
            max_items: 50_001,
            ..Settings::default()
        };
        assert!(high.validate().is_err());
    }

    #[test]
    #[allow(non_snake_case)]
    fn 热键格式错误被拒绝() {
        assert!(Settings {
            launcher_hotkey: String::new(),
            ..Settings::default()
        }
        .validate()
        .is_err());
        assert!(Settings {
            launcher_hotkey: "Space".to_string(),
            ..Settings::default()
        }
        .validate()
        .is_err());
        assert!(Settings {
            launcher_hotkey: "Alt++Space".to_string(),
            ..Settings::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    #[allow(non_snake_case)]
    fn 只有修饰键的热键被拒绝() {
        // 回归：录制器曾把 Win 键当成主键，产生「Ctrl+Super」缺主键的组合，
        // 注册必然失败且会被持久化，启动时反复回退。必须在校验层拦住。
        for bad in ["Ctrl+Super", "Ctrl+Shift", "Alt+Meta", "Shift+Win"] {
            assert!(
                Settings {
                    launcher_hotkey: bad.to_string(),
                    ..Settings::default()
                }
                .validate()
                .is_err(),
                "{bad} 应被拒绝"
            );
        }
        // 正常组合仍然通过
        for good in ["Super+Space", "Alt+Space", "Ctrl+Alt+K"] {
            let settings = Settings {
                launcher_hotkey: good.to_string(),
                ..Settings::default()
            };
            assert!(settings.validate().is_ok(), "{good} 应通过");
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 主题非法被拒绝() {
        assert!(Settings {
            theme: "purple".to_string(),
            ..Settings::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    #[allow(non_snake_case)]
    fn 排除应用空值被拒绝() {
        assert!(Settings {
            excluded_apps: vec![" KeePassXC.exe".to_string()],
            ..Settings::default()
        }
        .validate()
        .is_err());
        assert!(Settings {
            excluded_apps: vec![String::new()],
            ..Settings::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    #[allow(non_snake_case)]
    fn 旧JSON缺字段时补默认() {
        let json = r#"{"launcher_hotkey":"Alt+Space"}"#;
        let settings: Settings = serde_json::from_str(json).expect("parse partial json");
        assert_eq!(settings.max_items, 10_000);
        assert!(settings.record_image);
        assert!(!settings.restore_clipboard_after_paste);
        assert!(settings.validate().is_ok());
    }

    #[test]
    #[allow(non_snake_case)]
    fn 旧JSON含已删除字段时被忽略() {
        // 旧版本设置里存在 actions_hotkey（划词热键），升级后不应导致读取失败
        let json = r#"{"launcher_hotkey":"Alt+Space","clipboard_hotkey":"Alt+V","actions_hotkey":"Alt+X"}"#;
        let settings: Settings = serde_json::from_str(json).expect("parse legacy json");
        assert_eq!(settings.clipboard_hotkey, "Alt+V");
        assert!(settings.validate().is_ok());
    }

    #[test]
    #[allow(non_snake_case)]
    fn 设置保存读取往返() {
        let storage = crate::services::storage::ClipboardStorage::open_in_memory()
            .expect("open in-memory storage");
        let settings = Settings {
            max_items: 2000,
            excluded_apps: vec!["KeePassXC.exe".to_string()],
            restore_clipboard_after_paste: true,
            ..Settings::default()
        };

        storage.save_settings(&settings).expect("save settings");
        let loaded = storage.load_settings();
        assert_eq!(loaded, settings);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 两个热键相同被拒绝() {
        let settings = Settings {
            launcher_hotkey: "Alt+V".to_string(),
            clipboard_hotkey: "Alt+V".to_string(),
            ..Settings::default()
        };
        assert!(settings.validate().is_err());
    }

    /// IPC 契约测试：验证 Rust 序列化字段集合与前端 fixture 一致
    ///
    /// fixture 文件存储在 src/lib/contract/*.keys.json，前端测试读取同一份比对。
    /// 任一侧删除/重命名字段都会导致测试失败。
    #[test]
    #[allow(non_snake_case)]
    fn ClipboardItem字段集合与前端fixture一致() {
        let fixture = include_str!("../../src/lib/contract/ClipboardItem.keys.json");
        let expected: Vec<String> = serde_json::from_str(fixture).expect("parse fixture");

        let item = ClipboardItem {
            id: 1,
            kind: ClipKind::Text,
            preview: String::new(),
            source_app: None,
            created_at: 0,
            last_copied_at: 0,
            copy_count: 1,
            pinned: false,
            sensitive: false,
            truncated: false,
            byte_size: 0,
            thumb_path: None,
        };

        let value = serde_json::to_value(&item).expect("serialize");
        let mut actual: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
        actual.sort();

        let mut expected_sorted = expected;
        expected_sorted.sort();

        assert_eq!(actual, expected_sorted);
    }

    #[test]
    #[allow(non_snake_case)]
    fn ClipboardDetail字段集合与前端fixture一致() {
        let fixture = include_str!("../../src/lib/contract/ClipboardDetail.keys.json");
        let expected: Vec<String> = serde_json::from_str(fixture).expect("parse fixture");

        let detail = ClipboardDetail {
            id: 1,
            kind: ClipKind::Text,
            preview: String::new(),
            text_body: None,
            html: None,
            rtf: None,
            file_list: None,
            file_exists: None,
            blob_path: None,
            thumb_path: None,
            byte_size: 0,
            created_at: 0,
            last_copied_at: 0,
            copy_count: 1,
            pinned: false,
            truncated: false,
            source_app: None,
        };

        let value = serde_json::to_value(&detail).expect("serialize");
        let mut actual: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
        actual.sort();

        let mut expected_sorted = expected;
        expected_sorted.sort();

        assert_eq!(actual, expected_sorted);
    }

    #[test]
    #[allow(non_snake_case)]
    fn ClipboardListResult字段集合与前端fixture一致() {
        let fixture = include_str!("../../src/lib/contract/ClipboardListResult.keys.json");
        let expected: Vec<String> = serde_json::from_str(fixture).expect("parse fixture");

        let result = ClipboardListResult {
            items: vec![],
            total: 0,
        };

        let value = serde_json::to_value(&result).expect("serialize");
        let mut actual: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
        actual.sort();

        let mut expected_sorted = expected;
        expected_sorted.sort();

        assert_eq!(actual, expected_sorted);
    }

    #[test]
    #[allow(non_snake_case)]
    fn Settings字段集合与前端fixture一致() {
        let fixture = include_str!("../../src/lib/contract/Settings.keys.json");
        let expected: Vec<String> = serde_json::from_str(fixture).expect("parse fixture");

        let settings = Settings::default();

        let value = serde_json::to_value(&settings).expect("serialize");
        let mut actual: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
        actual.sort();

        let mut expected_sorted = expected;
        expected_sorted.sort();

        assert_eq!(actual, expected_sorted);
    }

    #[test]
    #[allow(non_snake_case)]
    fn AppIndexRebuildResult字段集合与前端fixture一致() {
        let fixture = include_str!("../../src/lib/contract/AppIndexRebuildResult.keys.json");
        let expected: Vec<String> = serde_json::from_str(fixture).expect("parse fixture");

        let result = AppIndexRebuildResult { count: 0 };

        let value = serde_json::to_value(result).expect("serialize");
        let mut actual: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
        actual.sort();

        let mut expected_sorted = expected;
        expected_sorted.sort();

        assert_eq!(actual, expected_sorted);
    }

    #[test]
    #[allow(non_snake_case)]
    fn LauncherItem字段集合与前端fixture一致() {
        let fixture = include_str!("../../src/lib/contract/LauncherItem.keys.json");
        let expected: Vec<String> = serde_json::from_str(fixture).expect("parse fixture");

        let item = LauncherItem {
            key: "a.exe".to_string(),
            title: "Alpha".to_string(),
            subtitle: None,
            kind: LauncherKind::App,
            icon: None,
            score: 1.0,
        };

        let value = serde_json::to_value(&item).expect("serialize");
        let mut actual: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
        actual.sort();

        let mut expected_sorted = expected;
        expected_sorted.sort();

        assert_eq!(actual, expected_sorted);
    }

    #[test]
    #[allow(non_snake_case)]
    fn LauncherQueryResult字段集合与前端fixture一致() {
        let fixture = include_str!("../../src/lib/contract/LauncherQueryResult.keys.json");
        let expected: Vec<String> = serde_json::from_str(fixture).expect("parse fixture");

        let result = LauncherQueryResult {
            items: vec![],
            hint: None,
        };

        let value = serde_json::to_value(&result).expect("serialize");
        let mut actual: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
        actual.sort();

        let mut expected_sorted = expected;
        expected_sorted.sort();

        assert_eq!(actual, expected_sorted);
    }
}
