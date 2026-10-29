// 设置 command
//
// 只做参数校验与编排；持久化在 services/storage.rs，副作用（热键 / 自启 / 粘贴选项）
// 在本文件编排。所有含阻塞的操作放在 spawn_blocking 内。

use crate::error::AppError;
use crate::models::Settings;
use crate::services::hotkey::{HotkeyAction, HotkeyManager};
use crate::services::paste::PasteOptions;
use crate::services::storage::{ClipboardStorage, DbPath};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

/// 从受管状态取数据库路径
fn db_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, AppError> {
    app.try_state::<DbPath>()
        .map(|state| state.as_path().to_path_buf())
        .ok_or_else(|| AppError::Storage("database path is not configured".to_string()))
}

/// `settings://changed` 事件载荷
#[derive(serde::Serialize, Clone)]
struct SettingsChanged {
    keys: Vec<String>,
    failed: Vec<String>,
}

/// 读取设置
#[tauri::command]
pub async fn settings_get(app: tauri::AppHandle) -> Result<Settings, AppError> {
    let path = db_path(&app)?;

    tauri::async_runtime::spawn_blocking(move || -> Result<Settings, AppError> {
        let storage = ClipboardStorage::open_readonly_at(&path)?;
        Ok(storage.load_settings())
    })
    .await
    .map_err(|e| AppError::Internal(format!("settings_get failed: {}", e)))?
}

/// 更新设置：与当前值合并后校验、保存，并应用副作用
#[tauri::command]
pub async fn settings_update(
    app: tauri::AppHandle,
    patch: serde_json::Value,
) -> Result<Settings, AppError> {
    let path = db_path(&app)?;

    tauri::async_runtime::spawn_blocking(move || -> Result<Settings, AppError> {
        let mut storage = ClipboardStorage::open_write_at(&path)?;

        // 先在事务外构造合并值并校验，避免脏值进入事务
        // 事务内会重读当前值并再次合并
        let (current, merged, changed) = storage.transaction_update_settings(|current| {
            let mut base = serde_json::to_value(current)
                .map_err(|e| AppError::Internal(format!("序列化设置失败：{}", e)))?;
            if let (Some(base_obj), Some(patch_obj)) = (base.as_object_mut(), patch.as_object()) {
                for (key, value) in patch_obj {
                    base_obj.insert(key.clone(), value.clone());
                }
            } else {
                return Err(AppError::SettingsInvalid("设置必须是对象".to_string()));
            }

            let merged: Settings = serde_json::from_value(base)
                .map_err(|_| AppError::SettingsInvalid("设置字段类型不正确".to_string()))?;
            merged.validate().map_err(AppError::SettingsInvalid)?;
            Ok(merged)
        })?;

        if changed.is_empty() {
            return Ok(current);
        }

        let failed = apply_side_effects(&app, &current, &merged, &changed)?;

        app.emit(
            "settings://changed",
            SettingsChanged {
                keys: changed,
                failed: failed.clone(),
            },
        )
        .map_err(|e| AppError::Internal(format!("广播设置变更失败：{}", e)))?;

        // 运行时占用导致热键没注册成功：单独通知，不阻断设置保存
        for accelerator in &failed {
            let _ = app.emit(
                "hotkey://failed",
                serde_json::json!({ "accelerator": accelerator }),
            );
        }

        Ok(merged)
    })
    .await
    .map_err(|e| AppError::Internal(format!("settings_update failed: {}", e)))?
}

/// 恢复默认设置（二次确认在前端做）
#[tauri::command]
pub async fn settings_reset(app: tauri::AppHandle) -> Result<Settings, AppError> {
    let default = Settings::default();
    let patch = serde_json::to_value(&default)
        .map_err(|e| AppError::Internal(format!("serialize defaults: {}", e)))?;

    settings_update(app, patch).await
}

/// 暂停 / 恢复记录（托盘临时暂停：不写持久化值）
///
/// `duration_s = None` 表示暂停直到重启；`Some(0)` 表示恢复记录。
#[tauri::command]
pub async fn settings_set_paused(
    app: tauri::AppHandle,
    duration_s: Option<i64>,
) -> Result<(), AppError> {
    let path = db_path(&app)?;

    tauri::async_runtime::spawn_blocking(move || -> Result<(), AppError> {
        let mut storage = ClipboardStorage::open_write_at(&path)?;
        let flags = app.state::<crate::services::clipboard::RuntimeFlags>();

        let mut keys = Vec::new();

        match duration_s {
            Some(0) => {
                // 恢复：清进程内暂停，并把持久化暂停关掉（事务）
                flags.set_pause_until(None);
                storage.transaction_update_settings(|current| {
                    if current.pause_recording {
                        let mut next = current.clone();
                        next.pause_recording = false;
                        Ok(next)
                    } else {
                        Ok(current.clone())
                    }
                })?;
                keys.push("pause_recording".to_string());
            }
            None => {
                // 暂停直到重启：仅进程内（用超长到期时间表示，重启后状态自然消失）
                let until = Instant::now() + Duration::from_secs(60 * 60 * 24 * 365);
                flags.set_pause_until(Some(until));
                keys.push("pause_recording".to_string());
            }
            Some(seconds) if seconds > 0 => {
                // 定时暂停：仅进程内，到期自动恢复，重启后不再暂停
                let until =
                    std::time::Instant::now() + std::time::Duration::from_secs(seconds as u64);
                flags.set_pause_until(Some(until));
                keys.push("pause_recording".to_string());
            }
            Some(_) => {
                return Err(AppError::SettingsInvalid("暂停时长必须为正数".to_string()));
            }
        }

        app.emit(
            "settings://changed",
            SettingsChanged {
                keys,
                failed: Vec::new(),
            },
        )
        .map_err(|e| AppError::Internal(format!("emit pause changed: {}", e)))?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(format!("settings_set_paused failed: {}", e)))?
}

/// 应用设置副作用；返回注册失败的热键 accelerator 列表
fn apply_side_effects(
    app: &tauri::AppHandle,
    previous: &Settings,
    next: &Settings,
    changed: &[String],
) -> Result<Vec<String>, AppError> {
    let mut failed = Vec::new();

    // 热键：逐个改绑（新键注册成功才注销旧键）
    if changed.contains(&"launcher_hotkey".to_string())
        && previous.launcher_hotkey != next.launcher_hotkey
        && !rebind_one(app, HotkeyAction::Launcher, &next.launcher_hotkey)?
    {
        failed.push(next.launcher_hotkey.clone());
    }
    if changed.contains(&"clipboard_hotkey".to_string())
        && previous.clipboard_hotkey != next.clipboard_hotkey
        && !rebind_one(app, HotkeyAction::Clipboard, &next.clipboard_hotkey)?
    {
        failed.push(next.clipboard_hotkey.clone());
    }
    if changed.contains(&"actions_hotkey".to_string())
        && previous.actions_hotkey != next.actions_hotkey
        && !rebind_one(app, HotkeyAction::Actions, &next.actions_hotkey)?
    {
        failed.push(next.actions_hotkey.clone());
    }

    // 开机自启：失败不阻断保存，与热键策略一致（返回结果由前端提示）
    if changed.contains(&"autostart".to_string()) && previous.autostart != next.autostart {
        let autolaunch = app.autolaunch();
        let result = if next.autostart {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
        if let Err(e) = result {
            log::warn!("设置开机自启失败：{}", e);
        }
    }

    // 粘贴后还原开关：同步到 PasteOptions
    if changed.contains(&"restore_clipboard_after_paste".to_string()) {
        if let Some(options) = app.try_state::<PasteOptions>() {
            options.set_restore_after_paste(next.restore_clipboard_after_paste);
        }
    }

    Ok(failed)
}

/// 改绑单个热键；true=成功
fn rebind_one(
    app: &tauri::AppHandle,
    action: HotkeyAction,
    accelerator: &str,
) -> Result<bool, AppError> {
    let manager = app.state::<HotkeyManager>();
    manager.rebind(app, action, accelerator)
}
