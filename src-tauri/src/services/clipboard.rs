// 剪贴板监听服务
//
// 后台线程轮询剪贴板序列号，检测到变化后读取内容并入库。
// 入库成功发送 `clipboard://changed`（节流 500ms），失败发送 `clipboard://insert-error`（不节流）。

use crate::error::AppError;
use crate::platform::clipboard::{get_sequence_number, read_clipboard_content, ClipboardEntry};
use crate::platform::focus::capture_foreground;
use crate::services::storage::ClipboardStorage;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// 事件节流窗口
const THROTTLE_WINDOW: Duration = Duration::from_millis(500);

/// 事件节流器
///
/// 500ms 内只发送一次事件；UI 收到事件后重新查询数据库，因此丢弃中间事件不会丢失数据。
/// 首条事件不节流（`last_emit` 初始为空）。
struct ThrottledEmitter {
    last_emit: Mutex<Option<Instant>>,
}

impl ThrottledEmitter {
    fn new() -> Self {
        Self {
            last_emit: Mutex::new(None),
        }
    }

    /// 发送事件；返回是否真正发送
    fn emit(
        &self,
        app: &AppHandle,
        event: &str,
        data: serde_json::Value,
    ) -> Result<bool, AppError> {
        let mut last = self
            .last_emit
            .lock()
            .map_err(|e| AppError::Internal(format!("throttle lock poisoned: {}", e)))?;

        let should_emit = match *last {
            Some(at) => at.elapsed() >= THROTTLE_WINDOW,
            None => true,
        };

        if should_emit {
            app.emit(event, data)
                .map_err(|e| AppError::Internal(format!("failed to emit {}: {}", event, e)))?;
            *last = Some(Instant::now());
        }

        Ok(should_emit)
    }
}

/// 剪贴板监听器
///
/// 管理后台监听线程的生命周期，Drop 时自动停止线程。
/// 必须由 `AppState` 持有，否则线程会被立即回收。
pub struct ClipboardMonitor {
    stop_flag: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ClipboardMonitor {
    /// 启动剪贴板监听
    pub fn start(app: AppHandle) -> Result<Self, AppError> {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();

        // 存储实例只属于监听线程，无需加锁
        let mut storage = ClipboardStorage::new()?;

        let handle = std::thread::spawn(move || {
            let mut last_seq = get_sequence_number().unwrap_or(0);
            let emitter = ThrottledEmitter::new();

            log::info!("clipboard monitor started");

            while !stop_clone.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));

                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }

                let current_seq = match get_sequence_number() {
                    Ok(seq) => seq,
                    Err(e) => {
                        log::warn!("get_sequence_number failed: {}", e);
                        continue;
                    }
                };

                // wrapping_sub 处理序列号回绕
                if current_seq.wrapping_sub(last_seq) == 0 {
                    continue;
                }
                last_seq = current_seq;

                // 读取剪贴板，失败时动态重试：立即 → 10ms → 50ms
                let entry = match read_with_retry(&stop_clone) {
                    Some(entry) => entry,
                    None => continue,
                };

                let mut entry = entry;
                attach_source_app(&mut entry);

                match storage.insert(&entry) {
                    Ok(outcome) => {
                        let payload = serde_json::json!({
                            "id": outcome.id(),
                            "kind": entry.content.kind(),
                        });

                        if let Err(e) = emitter.emit(&app, "clipboard://changed", payload) {
                            log::warn!("emit clipboard://changed failed: {}", e);
                        }

                        log::debug!(
                            "clipboard stored: id={}, kind={}, new={}",
                            outcome.id(),
                            entry.content.kind(),
                            outcome.is_new()
                        );
                    }
                    Err(e) => {
                        // 入库失败：立即通知 UI（不节流），载荷为 AppError 的 { code, message }
                        log::error!("clipboard insert failed: {}", e);
                        if let Err(emit_err) = app.emit("clipboard://insert-error", &e) {
                            log::warn!("emit clipboard://insert-error failed: {}", emit_err);
                        }
                    }
                }
            }

            log::info!("clipboard monitor stopped");
        });

        Ok(Self {
            stop_flag,
            handle: Some(handle),
        })
    }

    /// 停止监听线程并等待退出
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ClipboardMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 读取剪贴板内容，剪贴板被占用时重试
fn read_with_retry(stop_flag: &AtomicBool) -> Option<ClipboardEntry> {
    for delay in [0u64, 10, 50] {
        if stop_flag.load(Ordering::Relaxed) {
            return None;
        }
        if delay > 0 {
            std::thread::sleep(Duration::from_millis(delay));
        }

        match read_clipboard_content() {
            Ok(Some(entry)) => return Some(entry),
            Ok(None) => {
                // 没有可支持的格式（例如纯格式标记），忽略本次变化
                log::debug!("clipboard changed but no supported format");
                return None;
            }
            Err(AppError::ClipboardBusy) => continue,
            Err(e) => {
                log::warn!("failed to read clipboard: {}", e);
                return None;
            }
        }
    }

    log::warn!("clipboard busy after retries, skipped");
    None
}

/// 记录来源应用（进程名）。窗口标题暂不采集，避免引入隐私风险。
fn attach_source_app(entry: &mut ClipboardEntry) {
    if let Some(foreground) = capture_foreground() {
        entry.source_app = Some(foreground.process_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(non_snake_case)]
    fn 节流器初始为空() {
        let emitter = ThrottledEmitter::new();
        let guard = match emitter.last_emit.lock() {
            Ok(guard) => guard,
            Err(e) => panic!("lock poisoned: {}", e),
        };
        assert!(guard.is_none());
    }
}
