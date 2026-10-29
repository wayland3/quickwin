// 剪贴板查询 command
//
// 只做参数校验与编排；SQL 在 `services/storage.rs`。
// `rusqlite::Connection` 不是 `Sync`，因此不共享连接：每条 command 在
// `spawn_blocking` 内开短生命期连接，锁与连接都不跨 await。

use crate::error::AppError;
use crate::models::{ClipFilter, ClipboardDetail, ClipboardListResult, PasteMode};
use crate::services::clipboard::PasteSuppression;
use crate::services::paste::PasteOptions;
use crate::services::storage::{ClipboardStorage, DbPath};
use crate::services::window::OverlayManager;
use std::path::PathBuf;
use tauri::Manager;

/// 从受管状态取数据库路径
fn db_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, AppError> {
    app.try_state::<DbPath>()
        .map(|state| state.as_path().to_path_buf())
        .ok_or_else(|| AppError::Storage("database path is not configured".to_string()))
}

/// 列表查询（分页 + 搜索 + 筛选）
#[tauri::command]
pub async fn clipboard_list(
    app: tauri::AppHandle,
    query: String,
    filter: ClipFilter,
    limit: u32,
    offset: u32,
) -> Result<ClipboardListResult, AppError> {
    let path = db_path(&app)?;

    tauri::async_runtime::spawn_blocking(move || -> Result<ClipboardListResult, AppError> {
        let storage = ClipboardStorage::open_readonly_at(&path)?;
        let (items, total) = storage.list(&query, filter, limit, offset)?;
        Ok(ClipboardListResult { items, total })
    })
    .await
    .map_err(|e| AppError::Internal(format!("clipboard_list failed: {}", e)))?
}

/// 详情查询（正文、文件列表、blob 绝对路径）
#[tauri::command]
pub async fn clipboard_get(app: tauri::AppHandle, id: i64) -> Result<ClipboardDetail, AppError> {
    let path = db_path(&app)?;

    tauri::async_runtime::spawn_blocking(move || -> Result<ClipboardDetail, AppError> {
        let storage = ClipboardStorage::open_readonly_at(&path)?;
        storage
            .get(id)?
            .ok_or_else(|| AppError::Storage(format!("clipboard item not found: {}", id)))
    })
    .await
    .map_err(|e| AppError::Internal(format!("clipboard_get failed: {}", e)))?
}

/// 置顶 / 取消置顶
#[tauri::command]
pub async fn clipboard_set_pinned(
    app: tauri::AppHandle,
    id: i64,
    pinned: bool,
) -> Result<(), AppError> {
    let path = db_path(&app)?;

    tauri::async_runtime::spawn_blocking(move || -> Result<(), AppError> {
        let storage = ClipboardStorage::open_at(&path)?;
        storage.set_pinned(id, pinned)
    })
    .await
    .map_err(|e| AppError::Internal(format!("clipboard_set_pinned failed: {}", e)))?
}

/// 激活条目：写剪贴板 +（paste 时）发送 Ctrl+V
///
/// 整个流程含阻塞 Win32 调用，必须在 `spawn_blocking` 内执行。
#[tauri::command]
pub async fn clipboard_activate(
    app: tauri::AppHandle,
    id: i64,
    mode: PasteMode,
) -> Result<(), AppError> {
    let path = db_path(&app)?;

    tauri::async_runtime::spawn_blocking(move || -> Result<(), AppError> {
        let storage = ClipboardStorage::open_readonly_at(&path)?;
        let detail = storage
            .get(id)?
            .ok_or_else(|| AppError::Storage(format!("clipboard item not found: {}", id)))?;

        let manager = app.state::<OverlayManager>();
        let suppression = app.state::<PasteSuppression>();
        let options = app.state::<PasteOptions>();

        crate::services::paste::activate(&app, &manager, &suppression, &options, detail, mode)
    })
    .await
    .map_err(|e| AppError::Internal(format!("clipboard_activate failed: {}", e)))?
}

/// 设置「粘贴后还原原剪贴板」（设置页在 T-601 接入）
#[tauri::command]
pub fn clipboard_set_restore_after_paste(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<(), AppError> {
    let options = app
        .try_state::<PasteOptions>()
        .ok_or_else(|| AppError::Internal("paste options not configured".into()))?;
    options.set_restore_after_paste(enabled);
    Ok(())
}

/// 删除条目；`hard=true` 时连 blob 文件一起删
#[tauri::command]
pub async fn clipboard_delete(app: tauri::AppHandle, id: i64, hard: bool) -> Result<(), AppError> {
    let path = db_path(&app)?;

    tauri::async_runtime::spawn_blocking(move || -> Result<(), AppError> {
        let mut storage = ClipboardStorage::open_at(&path)?;
        storage.delete(id, hard)
    })
    .await
    .map_err(|e| AppError::Internal(format!("clipboard_delete failed: {}", e)))?
}
