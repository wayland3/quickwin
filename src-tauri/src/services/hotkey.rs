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

    pub fn register_defaults(&self, app: &AppHandle) {
        let defaults = vec![
            (HotkeyAction::Launcher, "Alt+Space"),
            (HotkeyAction::Clipboard, "Alt+V"),
            (HotkeyAction::Actions, "Alt+X"),
        ];

        for (action, accelerator) in defaults {
            if let Err(e) = self.register(app, action, accelerator) {
                // 单个失败不中断其余热键注册；失败记录可由前端 hotkey_status 主动拉取
                log::warn!("failed to register hotkey {}: {}", accelerator, e);
            }
        }
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

        // 新键与旧键相同：无需任何操作，避免重复注册同一 shortcut 导致误报失败
        if let Some(old_acc) = &old_accelerator {
            if old_acc == new_accelerator {
                return Ok(true);
            }
        }

        // register 失败会往 failed 列表追加记录；改绑冲突时旧键仍可用，需回滚该记录
        let failed_len = self.failed_len()?;

        // 先注册新键
        if self.register(app, action, new_accelerator).is_err() {
            log::warn!("failed to register new hotkey: {}", new_accelerator);
            self.truncate_failed(failed_len)?;
            return Ok(false);
        }

        // 注册成功后，注销旧的热键
        if let Some(old_acc) = old_accelerator {
            if old_acc != new_accelerator {
                let shortcut: Shortcut = old_acc
                    .parse()
                    .map_err(|_| AppError::Internal(format!("invalid accelerator: {}", old_acc)))?;

                if let Err(e) = app.global_shortcut().unregister(shortcut) {
                    // 注销失败：回滚管理器记录到旧键（保证 unregister(action) 仍可清理），
                    // 并在 failed 列表显式上报、补发事件，避免旧组合残留无人知晓
                    log::warn!("failed to unregister old hotkey {}: {}", old_acc, e);
                    self.set_binding(action, &old_acc)?;
                    self.truncate_failed(failed_len)?;
                    let item = FailedHotkey {
                        action,
                        accelerator: old_acc,
                        code: "hotkey.unregister_failed".to_string(),
                    };
                    let _ = app.emit("hotkey://failed", item.clone());
                    self.push_failed(item)?;
                    return Ok(true);
                }
            }
        }

        Ok(true)
    }

    /// 当前 failed 列表长度
    fn failed_len(&self) -> Result<usize, AppError> {
        self.failed
            .lock()
            .map(|list| list.len())
            .map_err(|_| AppError::Internal("lock poisoned".into()))
    }

    /// 截断 failed 列表到指定长度（改绑失败时回滚 register 追加的记录）
    fn truncate_failed(&self, len: usize) -> Result<(), AppError> {
        let mut list = self
            .failed
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))?;
        list.truncate(len);
        Ok(())
    }

    /// 直接写入绑定（旧键注销失败时回滚用）
    fn set_binding(&self, action: HotkeyAction, accelerator: &str) -> Result<(), AppError> {
        let mut bindings = self
            .bindings
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))?;
        bindings.insert(action, accelerator.to_string());
        Ok(())
    }

    /// 追加失败记录
    fn push_failed(&self, item: FailedHotkey) -> Result<(), AppError> {
        let mut list = self
            .failed
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))?;
        list.push(item);
        Ok(())
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

    /// 测试辅助：把 Result 的错误映射为 panic 消息（仅测试代码允许）
    trait UnwrapTest<T> {
        fn unwrap_or_fail(self, context: &str) -> T;
    }

    impl<T> UnwrapTest<T> for Result<T, AppError> {
        fn unwrap_or_fail(self, context: &str) -> T {
            match self {
                Ok(value) => value,
                Err(e) => panic!("{}: {}", context, e),
            }
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 热键动作序列化正确() {
        assert_eq!(HotkeyAction::Launcher.as_str(), "launcher");
        assert_eq!(HotkeyAction::Clipboard.as_str(), "clipboard");
        assert_eq!(HotkeyAction::Actions.as_str(), "actions");
    }

    #[test]
    #[allow(non_snake_case)]
    fn 失败列表可截断回滚() {
        let manager = HotkeyManager::new();
        let item = FailedHotkey {
            action: HotkeyAction::Launcher,
            accelerator: "Alt+Space".to_string(),
            code: "hotkey.register_failed".to_string(),
        };

        manager
            .push_failed(item.clone())
            .unwrap_or_fail("push first");
        manager.push_failed(item).unwrap_or_fail("push second");
        assert_eq!(manager.failed_len().unwrap_or_fail("failed len"), 2);

        manager.truncate_failed(1).unwrap_or_fail("truncate");
        assert_eq!(manager.failed_len().unwrap_or_fail("failed len"), 1);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 绑定可直接写入回滚() {
        let manager = HotkeyManager::new();
        manager
            .set_binding(HotkeyAction::Clipboard, "Alt+V")
            .unwrap_or_fail("set binding");
        let value = manager
            .bindings
            .lock()
            .map(|b| b.get(&HotkeyAction::Clipboard).cloned())
            .map_err(|e| AppError::Internal(format!("bindings lock: {}", e)))
            .unwrap_or_fail("bindings lock");
        assert_eq!(value, Some("Alt+V".to_string()));
    }
}
