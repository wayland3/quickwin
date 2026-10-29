// 启动器相关 command：应用索引重建
//
// 索引扫描与落库都是阻塞操作（COM / SQLite），统一放到 spawn_blocking，
// 避免占用 async 运行时线程。

use crate::error::AppError;
use crate::models::AppIndexRebuildResult;
use crate::services::appindex::{rebuild_index, IndexRebuildLock};
use crate::services::storage::DbPath;
use tauri::Manager;

/// 全量重建应用索引，返回写入条数（内容未变化时跳过写库）
#[tauri::command]
pub async fn app_index_rebuild(app: tauri::AppHandle) -> Result<AppIndexRebuildResult, AppError> {
    let db_path = app
        .try_state::<DbPath>()
        .map(|state| state.as_path().to_path_buf())
        .ok_or_else(|| AppError::Storage("数据库尚未就绪".to_string()))?;
    let lock = app
        .try_state::<IndexRebuildLock>()
        .map(|state| state.inner().clone())
        .unwrap_or_default();

    let count = tauri::async_runtime::spawn_blocking(move || rebuild_index(&db_path, &lock))
        .await
        .map_err(|error| {
            AppError::Internal(format!("app index rebuild join failed: {}", error))
        })??;

    Ok(AppIndexRebuildResult { count })
}
