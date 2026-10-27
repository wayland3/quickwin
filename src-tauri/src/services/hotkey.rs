// 热键服务
// 统一入口，业务侧只订阅动作事件

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HotkeyAction {
    Launcher,
    Clipboard,
    Actions,
}

impl HotkeyAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            HotkeyAction::Launcher => "launcher",
            HotkeyAction::Clipboard => "clipboard",
            HotkeyAction::Actions => "actions",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyBinding {
    pub action: HotkeyAction,
    pub accelerator: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HotkeyStatus {
    pub bindings: Vec<HotkeyBinding>,
    pub failed: Vec<FailedHotkey>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FailedHotkey {
    pub action: HotkeyAction,
    pub accelerator: String,
    pub code: String,
}

pub struct HotkeyManager {
    bindings: Arc<Mutex<HashMap<HotkeyAction, String>>>,
    failed: Arc<Mutex<Vec<FailedHotkey>>>,
}

impl HotkeyManager {
    pub fn new() -> Self {
        Self {
            bindings: Arc::new(Mutex::new(HashMap::new())),
            failed: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn register_defaults(&self, app: &AppHandle) -> Result<(), AppError> {
        let defaults = vec![
            (HotkeyAction::Launcher, "Alt+Space"),
            (HotkeyAction::Clipboard, "Alt+V"),
            (HotkeyAction::Actions, "Alt+X"),
        ];

        for (action, accelerator) in defaults {
            if let Err(e) = self.register(app, action, accelerator) {
                log::warn!("failed to register hotkey {}: {}", accelerator, e);
            }
        }

        Ok(())
    }

    pub fn register(
        &self,
        app: &AppHandle,
        action: HotkeyAction,
        accelerator: &str,
    ) -> Result<(), AppError> {
        let shortcut: Shortcut = accelerator
            .parse()
            .map_err(|_| AppError::Internal(format!("invalid accelerator: {}", accelerator)))?;

        let result = app.global_shortcut().on_shortcut(shortcut, {
            let app_clone = app.clone();
            let action_clone = action;
            move |_app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    let _ = app_clone.emit("hotkey://pressed", action_clone.as_str());
                }
            }
        });

        match result {
            Ok(_) => {
                let mut bindings = self
                    .bindings
                    .lock()
                    .map_err(|_| AppError::Internal("lock poisoned".into()))?;
                bindings.insert(action, accelerator.to_string());

                // 移除之前的失败记录（如果有）
                let mut failed = self
                    .failed
                    .lock()
                    .map_err(|_| AppError::Internal("lock poisoned".into()))?;
                failed.retain(|f| f.action != action);

                Ok(())
            }
            Err(_) => {
                let failed_hotkey = FailedHotkey {
                    action,
                    accelerator: accelerator.to_string(),
                    code: "hotkey.register_failed".to_string(),
                };

                let mut failed_list = self
                    .failed
                    .lock()
                    .map_err(|_| AppError::Internal("lock poisoned".into()))?;
                failed_list.push(failed_hotkey.clone());

                // 发送失败事件
                let _ = app.emit("hotkey://failed", failed_hotkey);

                Err(AppError::HotkeyRegisterFailed)
            }
        }
    }

    pub fn unregister(&self, app: &AppHandle, action: HotkeyAction) -> Result<(), AppError> {
        let mut bindings = self
            .bindings
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))?;

        if let Some(accelerator) = bindings.remove(&action) {
            let shortcut: Shortcut = accelerator
                .parse()
                .map_err(|_| AppError::Internal(format!("invalid accelerator: {}", accelerator)))?;

            app.global_shortcut()
                .unregister(shortcut)
                .map_err(|_| AppError::Internal("failed to unregister hotkey".into()))?;
        }

        Ok(())
    }

    pub fn rebind(
        &self,
        app: &AppHandle,
        action: HotkeyAction,
        new_accelerator: &str,
    ) -> Result<bool, AppError> {
        // 先保存旧的热键
        let old_accelerator = {
            let bindings = self
                .bindings
                .lock()
                .map_err(|_| AppError::Internal("lock poisoned".into()))?;
            bindings.get(&action).cloned()
        };

        // 先注册新键
        if let Err(e) = self.register(app, action, new_accelerator) {
            log::warn!("failed to register new hotkey: {}", e);
            return Ok(false);
        }

        // 注册成功后，注销旧的热键
        if let Some(old_acc) = old_accelerator {
            if old_acc != new_accelerator {
                let shortcut: Shortcut = old_acc
                    .parse()
                    .map_err(|_| AppError::Internal(format!("invalid accelerator: {}", old_acc)))?;

                let _ = app.global_shortcut().unregister(shortcut);
            }
        }

        Ok(true)
    }

    pub fn get_status(&self) -> Result<HotkeyStatus, AppError> {
        let bindings = self
            .bindings
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))?;

        let failed = self
            .failed
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))?;

        let binding_list: Vec<HotkeyBinding> = bindings
            .iter()
            .map(|(action, accelerator)| HotkeyBinding {
                action: *action,
                accelerator: accelerator.clone(),
            })
            .collect();

        Ok(HotkeyStatus {
            bindings: binding_list,
            failed: failed.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(non_snake_case)]
    fn 热键动作序列化正确() {
        assert_eq!(HotkeyAction::Launcher.as_str(), "launcher");
        assert_eq!(HotkeyAction::Clipboard.as_str(), "clipboard");
        assert_eq!(HotkeyAction::Actions.as_str(), "actions");
    }
}
