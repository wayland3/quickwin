// 启动器相关 command：应用索引重建
//
// 索引扫描与落库都是阻塞操作（COM / SQLite），统一放到 spawn_blocking，
// 避免占用 async 运行时线程。

use crate::error::AppError;
use crate::models::{AppIndexRebuildResult, LauncherQueryResult};
use crate::services::appindex::{rebuild_index, IndexRebuildLock};
use crate::services::ranking::{self, DEFAULT_LIMIT, MAX_LIMIT};
use crate::services::storage::{ClipboardStorage, DbPath, UsageStat};
use std::collections::HashMap;
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

/// 查询启动器结果
#[tauri::command]
pub async fn launcher_query(
    app: tauri::AppHandle,
    query: String,
    limit: Option<usize>,
) -> Result<LauncherQueryResult, AppError> {
    let db_path = app
        .try_state::<DbPath>()
        .map(|state| state.as_path().to_path_buf())
        .ok_or_else(|| AppError::Storage("数据库尚未就绪".to_string()))?;
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let result = tauri::async_runtime::spawn_blocking(move || {
        let storage = ClipboardStorage::open_readonly_at(&db_path)?;
        let rows = storage.load_app_index()?;
        let usage: HashMap<String, UsageStat> = storage
            .load_usage_stats()?
            .into_iter()
            .map(|stat| (stat.key.clone(), stat))
            .collect();

        let items = ranking::rank_apps(&query, &rows, &usage, now_ms(), limit);
        let items = ranking::with_web_fallback(items, &query);
        Ok::<_, AppError>(LauncherQueryResult { items, hint: None })
    })
    .await
    .map_err(|error| AppError::Internal(format!("launcher query join failed: {}", error)))??;

    Ok(result)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
