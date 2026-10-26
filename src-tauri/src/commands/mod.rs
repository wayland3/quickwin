use crate::error::AppError;
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
