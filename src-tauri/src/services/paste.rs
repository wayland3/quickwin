// 激活（粘贴 / 仅复制）编排层
//
// 顺序（PRD 6.4）：hide → restore → suppress.begin → write → suppress.seal → Ctrl+V。
// 全部阻塞调用（剪贴板 / SendInput / restore）只允许在 spawn_blocking 内执行。

use crate::error::AppError;
use crate::models::{ClipboardDetail, PasteMode};
use crate::platform::clipboard::{write_clipboard_content, ClipboardContent, ImageFormat};
use crate::platform::focus::ForegroundWindow;
use crate::platform::input::send_paste;
use crate::services::clipboard::PasteSuppression;
use crate::services::storage::content_hash;
use crate::services::window::{OverlayManager, RestoreOutcome};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

/// 粘贴后还原原剪贴板（默认关，持久化随 T-107）
pub struct PasteOptions {
    restore_after_paste: AtomicBool,
}

impl PasteOptions {
    pub fn new() -> Self {
        Self {
            restore_after_paste: AtomicBool::new(false),
        }
    }

    pub fn restore_after_paste(&self) -> bool {
        self.restore_after_paste.load(Ordering::Relaxed)
    }

    pub fn set_restore_after_paste(&self, enabled: bool) {
        self.restore_after_paste.store(enabled, Ordering::Relaxed);
    }
}

impl Default for PasteOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// 激活入口：写剪贴板 +（可选）发送 Ctrl+V；降级通过 `app://toast` 提示
pub fn activate(
    app: &AppHandle,
    manager: &OverlayManager,
    suppression: &PasteSuppression,
    options: &PasteOptions,
    detail: ClipboardDetail,
    mode: PasteMode,
) -> Result<(), AppError> {
    let content = detail_to_content(&detail)?;
    let hash = content_hash(&content)?;
    let window = app.get_webview_window("overlay");

    // 并发激活：已有流程在跑时直接拒绝，避免区间合并与焦点交叉
    let paste_lock = manager.paste_guard();
    let _guard = paste_lock
        .try_lock()
        .map_err(|_| AppError::Internal("粘贴操作正在进行，请稍候".into()))?;

    if mode == PasteMode::Copy {
        write_with_suppression(suppression, &content, hash)?;
        emit_toast(app, "已复制");
        return Ok(());
    }

    let prev = manager.prev_window();
    let prev = match prev {
        Some(prev) if !prev.is_elevated => Some(prev),
        _ => None,
    };

    // 没有可恢复的目标（未捕获 / 提权）：不隐藏、不发键，只保证剪贴板可用
    let restored = match (&window, &prev) {
        (Some(window), Some(_)) => match manager.hide_for_paste(window) {
            RestoreOutcome::Restored => true,
            other => {
                log::warn!("paste: restore outcome = {:?}", other);
                false
            }
        },
        (Some(_), None) => false,
        (None, _) => false,
    };

    // PRD：即使焦点恢复失败，也要保证剪贴板已就绪
    let snapshot = if options.restore_after_paste() {
        read_restorable_snapshot()
    } else {
        None
    };

    let seq_after = match write_with_suppression(suppression, &content, hash.clone()) {
        Ok(seq) => seq,
        Err(e) => {
            resume_and_toast(app, manager, &window, "复制失败，请重试");
            return Err(e);
        }
    };

    if !restored {
        resume_and_toast(app, manager, &window, "已复制，请手动粘贴");
        return Ok(());
    }

    if let Err(e) = send_paste() {
        log::warn!("send_paste failed: {}", e);
        resume_and_toast(app, manager, &window, "已复制，请手动粘贴");
        return Ok(());
    }

    if let Some((snapshot, snapshot_hash)) = snapshot {
        schedule_restore(
            paste_lock.clone(),
            suppression,
            snapshot,
            snapshot_hash,
            seq_after,
        );
    }

    Ok(())
}

/// 写剪贴板并登记抑制；返回写入后的序列号
fn write_with_suppression(
    suppression: &PasteSuppression,
    content: &ClipboardContent,
    hash: String,
) -> Result<u32, AppError> {
    use crate::platform::clipboard::get_sequence_number;

    let seq_before = get_sequence_number()?;
    suppression.begin(seq_before, Instant::now());
    write_clipboard_content(content)?;
    let seq_after = get_sequence_number()?;
    suppression.seal(seq_before, seq_after, Some(hash), Instant::now());
    Ok(seq_after)
}

/// 读当前剪贴板快照（不可还原或已截断时返回 None）
fn read_restorable_snapshot() -> Option<(ClipboardContent, String)> {
    use crate::platform::clipboard::read_clipboard_content;

    let entry = read_clipboard_content().ok().flatten()?;
    if entry.truncated {
        return None;
    }
    let hash = content_hash(&entry.content).ok()?;
    Some((entry.content, hash))
}

/// Ctrl+V 后 300ms 还原快照；若期间序列号又变（用户手动复制）或已有新激活则放弃
fn schedule_restore(
    lock: Arc<Mutex<()>>,
    suppression: &PasteSuppression,
    content: ClipboardContent,
    hash: String,
    expected_seq: u32,
) {
    let suppression = suppression.handle();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));

        // 还原写同样纳入 paste_lock 序列化，避免与下一次激活互相覆盖
        let Ok(_guard) = lock.try_lock() else {
            log::debug!("restore clipboard skipped: paste in progress");
            return;
        };

        match crate::platform::clipboard::get_sequence_number() {
            Ok(seq) if seq == expected_seq => {
                if let Err(e) = write_with_suppression(&suppression, &content, hash) {
                    log::warn!("restore clipboard failed: {}", e);
                }
            }
            Ok(_) => log::debug!("restore clipboard skipped: sequence changed"),
            Err(e) => log::warn!("restore clipboard: sequence read failed: {}", e),
        }
    });
}

/// 降级：面板已隐藏时重新展示 + toast
fn resume_and_toast(
    app: &AppHandle,
    manager: &OverlayManager,
    window: &Option<WebviewWindow>,
    message: &str,
) {
    if manager.get_state() == crate::services::window::OverlayState::Hidden {
        if let Some(window) = window {
            if let Err(e) = manager.resume_clipboard(window) {
                log::warn!("resume_clipboard failed: {}", e);
            }
        }
    }
    emit_toast(app, message);
}

fn emit_toast(app: &AppHandle, message: &str) {
    let payload = serde_json::json!({ "level": "info", "message": message });
    if let Err(e) = app.emit("app://toast", payload) {
        log::warn!("emit app://toast failed: {}", e);
    }
}

/// 详情 → 可写回内容
pub fn detail_to_content(detail: &ClipboardDetail) -> Result<ClipboardContent, AppError> {
    match detail.kind {
        crate::models::ClipKind::Text => detail
            .text_body
            .clone()
            .map(ClipboardContent::Text)
            .ok_or(AppError::ClipboardRead),
        crate::models::ClipKind::Html => detail
            .html
            .clone()
            .map(ClipboardContent::Html)
            .ok_or(AppError::ClipboardRead),
        crate::models::ClipKind::Rtf => detail
            .rtf
            .clone()
            .map(ClipboardContent::Rtf)
            .ok_or(AppError::ClipboardRead),
        crate::models::ClipKind::Files => detail
            .file_list
            .clone()
            .filter(|files| !files.is_empty())
            .map(ClipboardContent::Files)
            .ok_or(AppError::ClipboardRead),
        crate::models::ClipKind::Image => {
            let path = detail
                .blob_path
                .as_deref()
                .filter(|p| !p.is_empty())
                .ok_or(AppError::ClipboardRead)?;
            let data = std::fs::read(Path::new(path)).map_err(|e| {
                AppError::Internal(format!(
                    "read blob failed (len={}): {}",
                    detail.byte_size, e
                ))
            })?;
            let format = if path.to_ascii_lowercase().ends_with(".dib") {
                ImageFormat::Dib
            } else {
                ImageFormat::Png
            };
            Ok(ClipboardContent::Image { data, format })
        }
    }
}

/// 提权预检（供 command 层日志/分支复用）
pub fn is_elevated(prev: &Option<ForegroundWindow>) -> bool {
    prev.as_ref().map(|p| p.is_elevated).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ClipKind, ClipboardDetail};

    fn detail(kind: ClipKind) -> ClipboardDetail {
        ClipboardDetail {
            id: 1,
            kind,
            preview: String::new(),
            text_body: None,
            html: None,
            rtf: None,
            file_list: None,
            file_exists: None,
            blob_path: None,
            thumb_path: None,
            byte_size: 0,
            created_at: 0,
            last_copied_at: 0,
            copy_count: 1,
            pinned: false,
            truncated: false,
            source_app: None,
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 文本详情转为文本内容() {
        let mut item = detail(ClipKind::Text);
        item.text_body = Some("你好".to_string());
        match detail_to_content(&item) {
            Ok(ClipboardContent::Text(text)) => assert_eq!(text, "你好"),
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 缺少正文时返回读取错误() {
        assert!(detail_to_content(&detail(ClipKind::Text)).is_err());
        assert!(detail_to_content(&detail(ClipKind::Image)).is_err());
    }
}
