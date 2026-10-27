use crate::error::AppError;
use crate::services::window::{OverlayManager, OverlayMode, ResizePayload, ShowPayload};
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
    overlay_manager.hide(&window)
}

#[tauri::command]
pub async fn overlay_resize(window: Window, payload: ResizePayload) -> Result<(), AppError> {
    let overlay_manager = window.state::<OverlayManager>();
    overlay_manager.resize(&window, payload.height)
}
