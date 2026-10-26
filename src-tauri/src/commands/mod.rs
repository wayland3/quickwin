use crate::error::AppError;

#[tauri::command]
pub async fn app_ping() -> Result<String, AppError> {
    Ok("pong".to_string())
}
