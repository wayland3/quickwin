// 剪贴板监听服务
//
// 后台线程轮询剪贴板序列号，检测到变化后发送事件。

use crate::error::AppError;
use crate::platform::clipboard::{get_sequence_number, open_clipboard};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use tauri::{AppHandle, Emitter};

/// 剪贴板监听器
///
/// 管理后台监听线程的生命周期。
/// Drop 时自动停止线程。
pub struct ClipboardMonitor {
    stop_flag: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ClipboardMonitor {
    /// 启动剪贴板监听
    ///
    /// 创建后台线程，每 250ms 轮询剪贴板序列号。
    /// 检测到变化后发送 `clipboard://changed` 事件。
    pub fn start(app: AppHandle) -> Result<Self, AppError> {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();

        let handle = std::thread::spawn(move || {
            let mut last_seq = get_sequence_number().unwrap_or(0);
            let mut fail_count = 0u32;

            log::info!("clipboard monitor started");

            while !stop_clone.load(Ordering::Relaxed) {
                // 轮询间隔 250ms
                std::thread::sleep(std::time::Duration::from_millis(250));

                // 检查停止标志
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }

                // 获取当前序列号
                let current_seq = match get_sequence_number() {
                    Ok(seq) => seq,
                    Err(e) => {
                        log::warn!("get_sequence_number failed: {}", e);
                        continue;
                    }
                };

                // 使用 wrapping_sub 处理回绕
                let diff = current_seq.wrapping_sub(last_seq);
                if diff == 0 {
                    // 无变化
                    continue;
                }

                // 检测到变化，尝试读取剪贴板
                // 动态重试：立即 → 10ms → 50ms
                let retry_delays = [0, 10, 50];
                let mut opened = false;

                for &delay in &retry_delays {
                    // 检查停止标志
                    if stop_clone.load(Ordering::Relaxed) {
                        break;
                    }

                    if delay > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(delay));
                    }

                    match open_clipboard() {
                        Ok(_guard) => {
                            // 成功打开，立即关闭（RAII 自动）
                            opened = true;
                            break;
                        }
                        Err(_) => {
                            // 失败，继续重试
                            continue;
                        }
                    }
                }

                if opened {
                    // 更新序列号
                    last_seq = current_seq;
                    fail_count = 0;

                    // 发送事件（只带 id 与类型，不带内容）
                    let event_id = current_seq;
                    let _ = app.emit(
                        "clipboard://changed",
                        serde_json::json!({
                            "id": event_id,
                            "type": "unknown" // TODO: T-202 实现类型检测
                        }),
                    );

                    log::debug!("clipboard changed: id={}", event_id);
                } else {
                    // 重试失败，记录警告
                    fail_count += 1;
                    log::warn!(
                        "clipboard open failed after retries, skip this change (fail_count={})",
                        fail_count
                    );
                    // 仍然更新序列号，避免重复尝试
                    last_seq = current_seq;
                }
            }

            log::info!("clipboard monitor stopped");
        });

        Ok(Self {
            stop_flag,
            handle: Some(handle),
        })
    }

    /// 停止监听线程
    ///
    /// 设置停止标志并等待线程退出。
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
