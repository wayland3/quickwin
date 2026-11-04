// 热键服务
// 统一入口，业务侧只订阅动作事件

use crate::error::AppError;
use global_hotkey::GlobalHotKeyManager;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HotkeyAction {
    Launcher,
    Clipboard,
}

impl HotkeyAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            HotkeyAction::Launcher => "launcher",
            HotkeyAction::Clipboard => "clipboard",
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

/// 热键可用性实测结果
///
/// 探测方式：用**独立**的 `global_hotkey::GlobalHotKeyManager` 尝试注册该组合键。
/// 独立 manager 与运行时绑定（走 `tauri-plugin-global-shortcut`）互不干扰，
/// 因此探测不会与已注册的热键冲突，探测结束后组合键立即释放。
///
/// - `available`：注册成功（探测后立即注销）
/// - `occupied`：已被系统或其他程序占用
/// - `invalid`：accelerator 格式非法
///
/// **探测本身失败**（如 X11 连接断开、权限不足）不属于结论，直接返回 `Err`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HotkeyProbeReason {
    Available,
    Occupied,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyProbeResult {
    pub accelerator: String,
    pub available: bool,
    pub reason: HotkeyProbeReason,
    /// 面向用户的中文说明
    pub message: String,
}

pub struct HotkeyManager {
    bindings: Arc<Mutex<HashMap<HotkeyAction, String>>>,
    failed: Arc<Mutex<Vec<FailedHotkey>>>,
    suspended: Arc<Mutex<HashSet<HotkeyAction>>>,
}

impl HotkeyManager {
    pub fn new() -> Self {
        Self {
            bindings: Arc::new(Mutex::new(HashMap::new())),
            failed: Arc::new(Mutex::new(Vec::new())),
            suspended: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn register(
        &self,
        app: &AppHandle,
        action: HotkeyAction,
        accelerator: &str,
    ) -> Result<(), AppError> {
        log::info!(
            "register: action={:?}, accelerator='{}'",
            action,
            accelerator
        );

        let shortcut: Shortcut = accelerator.parse().map_err(|e| {
            log::error!("register: parse failed: {:?}", e);
            AppError::Internal(format!("invalid accelerator: {}", accelerator))
        })?;

        log::info!("register: parsed shortcut successfully");

        let suspended_clone = self.suspended.clone();
        let result = app.global_shortcut().on_shortcut(shortcut, {
            let app_clone = app.clone();
            let action_clone = action;
            move |_app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    // 检查是否被暂停（录制态时避免误触发全局热键）
                    let is_suspended = suspended_clone
                        .lock()
                        .ok()
                        .map(|s| s.contains(&action_clone))
                        .unwrap_or(false);
                    if !is_suspended {
                        let _ = app_clone.emit("hotkey://pressed", action_clone.as_str());
                    }
                }
            }
        });

        match result {
            Ok(_) => {
                log::info!("register: on_shortcut succeeded");
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
            Err(e) => {
                log::error!("register: on_shortcut failed: {:?}", e);
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

    /// 暂停指定热键（录制态使用，避免误触发）
    pub fn suspend(&self, action: HotkeyAction) -> Result<(), AppError> {
        let mut set = self
            .suspended
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))?;
        set.insert(action);
        Ok(())
    }

    /// 恢复指定热键
    pub fn resume(&self, action: HotkeyAction) -> Result<(), AppError> {
        let mut set = self
            .suspended
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))?;
        set.remove(&action);
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

/// 校验 accelerator 是否可解析（纯逻辑，便于单测）
///
/// 返回 `Ok(shortcut)` 或 `Err(中文原因)`。
fn parse_shortcut(accelerator: &str) -> Result<Shortcut, String> {
    let trimmed = accelerator.trim();
    if trimmed.is_empty() {
        return Err("热键未设置".to_string());
    }
    trimmed
        .parse::<Shortcut>()
        .map_err(|_| format!("{} 不是有效的组合键", trimmed))
}

/// 实测某个 accelerator 当前是否可用
///
/// 实测方式：用**独立**的 `global_hotkey::GlobalHotKeyManager` 尝试注册该组合键，
/// 再立即注销。独立 manager 与运行时绑定（走 `tauri-plugin-global-shortcut`）
/// 是两套实例，因此探测不会干扰运行时已注册的热键，探测结束后组合键立即释放。
///
/// 结论与错误的分界：
/// - 组合键被别的程序占用 → `Ok(reason: Occupied)`（这是**结论**，不是错误）
/// - 组合键格式非法 → `Ok(reason: Invalid)`（这是**结论**）
/// - 探测环境不可用（X11 连接断开、权限不足等）→ `Err(AppError)`（这是**失败**）
///
/// **自身绑定的特殊处理**：独立 manager 与运行时绑定是两套实例，但底层 OS 的
/// 全局热键表是**同一份**——运行时已注册的组合键，在探测时同样会返回
/// `AlreadyRegistered`。这不是「被别人占用」，而是「本应用自己占用」，
/// 因此必须先比对自身绑定，否则用户会看到自己正在用的热键被标红为「被占用」。
///
/// 探测是阻塞操作（底层要同步注册/注销全局热键），因此本函数是同步的，
/// 调用方必须放到专用线程（见 `commands::hotkey_probe`）。
impl HotkeyManager {
    pub fn probe(&self, accelerator: &str) -> Result<HotkeyProbeResult, AppError> {
        let trimmed = accelerator.trim();

        let shortcut = match parse_shortcut(trimmed) {
            Ok(s) => s,
            Err(reason) => {
                return Ok(HotkeyProbeResult {
                    accelerator: trimmed.to_string(),
                    available: false,
                    reason: HotkeyProbeReason::Invalid,
                    message: reason,
                });
            }
        };

        // 独立 manager：与运行时绑定是两套实例，但底层 OS 全局热键表是同一份，
        // 因此运行时已注册的组合键在探测时也会返回 AlreadyRegistered。
        // 先比对自身绑定，把「自己占用」与「被别人占用」区分开。
        if self.is_bound(trimmed) {
            return Ok(HotkeyProbeResult {
                accelerator: trimmed.to_string(),
                available: true,
                reason: HotkeyProbeReason::Available,
                message: "当前可用（本应用已绑定）".to_string(),
            });
        }

        let manager = GlobalHotKeyManager::new()
            .map_err(|e| AppError::HotkeyProbeFailed(format!("无法检测热键占用：{}", e)))?;

        match manager.register(shortcut) {
            Ok(_) => {
                // 探测成功立刻释放，避免长期占用该组合键
                let _ = manager.unregister(shortcut);
                Ok(HotkeyProbeResult {
                    accelerator: trimmed.to_string(),
                    available: true,
                    reason: HotkeyProbeReason::Available,
                    message: "当前可用".to_string(),
                })
            }
            // 已被系统或其他程序占用：这是结论，不是错误
            Err(global_hotkey::Error::AlreadyRegistered(_)) => Ok(HotkeyProbeResult {
                accelerator: trimmed.to_string(),
                available: false,
                reason: HotkeyProbeReason::Occupied,
                message: format!("{} 已被系统或其他程序占用", trimmed),
            }),
            // 其余均为探测环境故障：直接报错
            Err(e) => Err(AppError::HotkeyProbeFailed(format!(
                "无法检测热键占用：{}",
                e
            ))),
        }
    }

    /// 判断某个 accelerator 是否是本应用当前已注册的绑定
    ///
    /// 两边都做 `trim` 后比较：`bindings` 存的是注册时的原始字符串，
    /// 而探测入参会先 trim，不归一化会把「自己绑定的键」误判成「被别人占用」。
    fn is_bound(&self, accelerator: &str) -> bool {
        match self.bindings.lock() {
            Ok(bindings) => bindings
                .values()
                .any(|bound| bound.trim() == accelerator.trim()),
            Err(_) => false,
        }
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

    /// 测试辅助：把 Err 的错误映射为 panic 消息（仅测试代码允许）
    trait UnwrapErrTest {
        fn unwrap_err_or_fail(self, context: &str) -> String;
    }

    impl UnwrapErrTest for Result<Shortcut, String> {
        fn unwrap_err_or_fail(self, context: &str) -> String {
            match self {
                Ok(_) => panic!("{}: 期望错误，实际成功", context),
                Err(e) => e,
            }
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 热键动作序列化正确() {
        assert_eq!(HotkeyAction::Launcher.as_str(), "launcher");
        assert_eq!(HotkeyAction::Clipboard.as_str(), "clipboard");
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

    #[test]
    #[allow(non_snake_case)]
    fn 暂停与恢复热键() {
        let manager = HotkeyManager::new();

        // 初始状态：未暂停
        let suspended = manager.suspended.lock().unwrap();
        assert!(!suspended.contains(&HotkeyAction::Launcher));
        drop(suspended);

        // 暂停
        manager
            .suspend(HotkeyAction::Launcher)
            .unwrap_or_fail("suspend");
        let suspended = manager.suspended.lock().unwrap();
        assert!(suspended.contains(&HotkeyAction::Launcher));
        drop(suspended);

        // 恢复
        manager
            .resume(HotkeyAction::Launcher)
            .unwrap_or_fail("resume");
        let suspended = manager.suspended.lock().unwrap();
        assert!(!suspended.contains(&HotkeyAction::Launcher));
    }

    #[test]
    #[allow(non_snake_case)]
    fn 重复暂停幂等() {
        let manager = HotkeyManager::new();

        manager
            .suspend(HotkeyAction::Clipboard)
            .unwrap_or_fail("suspend 1");
        manager
            .suspend(HotkeyAction::Clipboard)
            .unwrap_or_fail("suspend 2");

        let suspended = manager.suspended.lock().unwrap();
        assert!(suspended.contains(&HotkeyAction::Clipboard));
        drop(suspended);

        // 一次恢复即可
        manager
            .resume(HotkeyAction::Clipboard)
            .unwrap_or_fail("resume");
        let suspended = manager.suspended.lock().unwrap();
        assert!(!suspended.contains(&HotkeyAction::Clipboard));
    }

    #[test]
    #[allow(non_snake_case)]
    fn 探测结果字段集合与前端fixture一致() {
        let fixture = include_str!("../../../src/lib/contract/HotkeyProbeResult.keys.json");
        let expected: Vec<String> = serde_json::from_str(fixture).expect("parse fixture");

        let result = HotkeyProbeResult {
            accelerator: "Super+Space".to_string(),
            available: true,
            reason: HotkeyProbeReason::Available,
            message: "当前可用".to_string(),
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
    fn 探测原因序列化为小写英文() {
        let value = serde_json::to_value(HotkeyProbeReason::Available).expect("serialize");
        assert_eq!(value, serde_json::json!("available"));
        let value = serde_json::to_value(HotkeyProbeReason::Occupied).expect("serialize");
        assert_eq!(value, serde_json::json!("occupied"));
        let value = serde_json::to_value(HotkeyProbeReason::Invalid).expect("serialize");
        assert_eq!(value, serde_json::json!("invalid"));
    }

    #[test]
    #[allow(non_snake_case)]
    fn 探测格式非法的组合键返回不可用结论() {
        let manager = HotkeyManager::new();

        let result = manager.probe("Bogus+Key").unwrap_or_fail("probe invalid");
        assert_eq!(result.reason, HotkeyProbeReason::Invalid);
        assert!(!result.available);
        assert!(result.message.contains("不是有效的组合键"));
    }

    #[test]
    #[allow(non_snake_case)]
    fn 探测空字符串返回不可用结论() {
        let manager = HotkeyManager::new();

        let result = manager.probe("   ").unwrap_or_fail("probe empty");
        assert_eq!(result.reason, HotkeyProbeReason::Invalid);
        assert!(!result.available);
        assert_eq!(result.message, "热键未设置");
    }

    #[test]
    #[allow(non_snake_case)]
    fn 自身绑定的组合键判定为已绑定() {
        let manager = HotkeyManager::new();
        manager
            .set_binding(HotkeyAction::Launcher, "Super+Space")
            .unwrap_or_fail("set binding");

        assert!(manager.is_bound("Super+Space"));
        // 两边都 trim，避免把「自己绑定的键」误判成「被别人占用」
        assert!(manager.is_bound(" Super+Space "));
        assert!(manager.is_bound("Super+Space "));
        assert!(!manager.is_bound("Alt+Space"));
    }

    #[test]
    #[allow(non_snake_case)]
    fn 探测自身绑定的组合键返回可用结论() {
        let manager = HotkeyManager::new();
        manager
            .set_binding(HotkeyAction::Launcher, "Super+Space")
            .unwrap_or_fail("set binding");

        // 即便底层 OS 里该组合键已被本应用占用，也应报「可用」而非「被占用」
        let result = manager
            .probe("Super+Space")
            .unwrap_or_fail("probe own binding");
        assert_eq!(result.reason, HotkeyProbeReason::Available);
        assert!(result.available);
        assert!(result.message.contains("本应用已绑定"));
    }

    #[test]
    #[allow(non_snake_case)]
    fn 探测的组合键格式校验() {
        // 合法组合键（含 Win/Super）
        assert!(parse_shortcut("Super+Space").is_ok());
        assert!(parse_shortcut("Alt+Space").is_ok());
        assert!(parse_shortcut("Ctrl+Alt+Shift+Super+K").is_ok());

        // 空值
        let err = parse_shortcut("").unwrap_err_or_fail("empty");
        assert_eq!(err, "热键未设置");
        let err = parse_shortcut("   ").unwrap_err_or_fail("blank");
        assert_eq!(err, "热键未设置");

        // 非法组合键
        let err = parse_shortcut("Bogus+Key").unwrap_err_or_fail("bogus");
        assert!(err.contains("不是有效的组合键"));
    }
}
