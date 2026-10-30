use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("剪贴板暂时被占用，请稍后重试")]
    ClipboardBusy,

    #[error("无法读取剪贴板内容")]
    ClipboardRead,

    #[error("目标窗口权限较高，无法自动操作")]
    ElevatedTarget,

    #[error("内容过长（超过 256KB）")]
    TooLarge,

    #[error("热键 {0} 已被其他程序占用")]
    HotkeyConflict(String),

    #[error("热键注册失败，请尝试更换")]
    HotkeyRegisterFailed,

    #[error("数据库操作失败")]
    Storage(String),

    #[error("设置值不合法")]
    SettingsInvalid(String),

    #[error("{0}")]
    LaunchFailed(String),

    #[error("发生内部错误，请查看日志")]
    Internal(String),
}

impl From<tauri::Error> for AppError {
    fn from(err: tauri::Error) -> Self {
        AppError::Internal(err.to_string())
    }
}

impl From<windows::core::Error> for AppError {
    fn from(err: windows::core::Error) -> Self {
        AppError::Internal(err.to_string())
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (code, message) = match self {
            AppError::ClipboardBusy => ("clipboard.busy", self.to_string()),
            AppError::ClipboardRead => ("clipboard.read_failed", self.to_string()),
            AppError::ElevatedTarget => ("focus.elevated_target", self.to_string()),
            AppError::TooLarge => ("content.too_large", self.to_string()),
            AppError::HotkeyConflict(key) => {
                ("hotkey.conflict", format!("热键 {} 已被其他程序占用", key))
            }
            AppError::HotkeyRegisterFailed => ("hotkey.register_failed", self.to_string()),
            AppError::Storage(_) => ("storage.error", self.to_string()),
            AppError::SettingsInvalid(reason) => {
                ("settings.invalid", format!("设置不合法：{}", reason))
            }
            AppError::LaunchFailed(reason) => ("launcher.launch_failed", reason.clone()),
            AppError::Internal(_) => ("internal", self.to_string()),
        };

        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("code", code)?;
        state.serialize_field("message", &message)?;
        state.end()
    }
}
