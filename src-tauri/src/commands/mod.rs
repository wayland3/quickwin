pub mod clipboard;
pub mod launcher;
pub mod settings;

use crate::error::AppError;
use crate::services::hotkey::{HotkeyAction, HotkeyManager, HotkeyStatus};
use crate::services::window::{
    HideReason, OverlayManager, OverlayMode, ResizePayload, ShowPayload,
};
use tauri::{Manager, Window};
use tauri_plugin_autostart::ManagerExt;

#[tauri::command]
pub async fn app_ping() -> Result<String, AppError> {
    Ok("pong".to_string())
}

#[tauri::command]
pub async fn autostart_enable<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), AppError> {
    let manager = app.autolaunch();
    manager
        .enable()
        .map_err(|e| AppError::Internal(format!("failed to enable autostart: {}", e)))
}

#[tauri::command]
pub async fn autostart_disable<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), AppError> {
    let manager = app.autolaunch();
    manager
        .disable()
        .map_err(|e| AppError::Internal(format!("failed to disable autostart: {}", e)))
}

#[tauri::command]
pub async fn autostart_is_enabled<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<bool, AppError> {
    let manager = app.autolaunch();
    manager
        .is_enabled()
        .map_err(|e| AppError::Internal(format!("failed to check autostart: {}", e)))
}

#[tauri::command]
pub async fn overlay_show(window: Window, payload: ShowPayload) -> Result<(), AppError> {
    let mode = OverlayMode::from_str(&payload.mode)
        .ok_or_else(|| AppError::Internal(format!("invalid mode: {}", payload.mode)))?;

    let overlay_manager = window.state::<OverlayManager>();
    overlay_manager.show(&window, mode)
}

#[tauri::command]
pub async fn overlay_hide(window: Window) -> Result<(), AppError> {
    let overlay_manager = window.state::<OverlayManager>();
    overlay_manager.hide(&window, HideReason::UserCommand)
}

#[tauri::command]
pub async fn overlay_resize(window: Window, payload: ResizePayload) -> Result<(), AppError> {
    let overlay_manager = window.state::<OverlayManager>();
    overlay_manager.resize(&window, payload.height)
}

#[tauri::command]
pub async fn hotkey_status(window: Window) -> Result<HotkeyStatus, AppError> {
    let hotkey_manager = window.state::<HotkeyManager>();
    hotkey_manager.get_status()
}

#[tauri::command]
pub async fn hotkey_rebind(
    app: tauri::AppHandle,
    action: String,
    accelerator: String,
) -> Result<bool, AppError> {
    let hotkey_manager = app.state::<HotkeyManager>();
    let action_enum = match action.as_str() {
        "launcher" => HotkeyAction::Launcher,
        "clipboard" => HotkeyAction::Clipboard,
        _ => return Err(AppError::Internal(format!("invalid action: {}", action))),
    };

    let success = hotkey_manager.rebind(&app, action_enum, &accelerator)?;

    // 改绑成功后同步写入 settings（保持 settings 与运行时一致）
    if success {
        let path = app
            .try_state::<crate::services::storage::DbPath>()
            .map(|state| state.as_path().to_path_buf());

        if let Some(path) = path {
            let action_clone = action.clone();
            let accelerator_clone = accelerator.clone();

            tauri::async_runtime::spawn(async move {
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    let mut storage =
                        crate::services::storage::ClipboardStorage::open_write_at(&path)?;
                    let _ = storage.transaction_update_settings(|current| {
                        let mut next = current.clone();
                        match action_clone.as_str() {
                            "launcher" => next.launcher_hotkey = accelerator_clone,
                            "clipboard" => next.clipboard_hotkey = accelerator_clone,
                            _ => {}
                        }
                        Ok(next)
                    });
                    Ok::<(), AppError>(())
                })
                .await;
            });
        }
    }

    Ok(success)
}

#[tauri::command]
pub async fn hotkey_suspend(app: tauri::AppHandle, action: String) -> Result<(), AppError> {
    let hotkey_manager = app.state::<HotkeyManager>();
    let action_enum = match action.as_str() {
        "launcher" => HotkeyAction::Launcher,
        "clipboard" => HotkeyAction::Clipboard,
        _ => return Err(AppError::Internal(format!("invalid action: {}", action))),
    };
    hotkey_manager.suspend(action_enum)
}

#[tauri::command]
pub async fn hotkey_resume(app: tauri::AppHandle, action: String) -> Result<(), AppError> {
    let hotkey_manager = app.state::<HotkeyManager>();
    let action_enum = match action.as_str() {
        "launcher" => HotkeyAction::Launcher,
        "clipboard" => HotkeyAction::Clipboard,
        _ => return Err(AppError::Internal(format!("invalid action: {}", action))),
    };
    hotkey_manager.resume(action_enum)
}
