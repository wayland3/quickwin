// 剪贴板 Win32 封装
//
// 所有剪贴板操作必须通过此模块，禁止在业务代码中直接调用 Win32 API。

use crate::error::AppError;
use windows::Win32::System::DataExchange::{CloseClipboard, OpenClipboard};

/// 获取剪贴板序列号
///
/// 用于检测剪贴板变化。每次剪贴板内容改变，序列号都会递增。
/// 序列号为 u32，会在 u32::MAX 后回绕到 0，使用 wrapping_sub 处理。
pub fn get_sequence_number() -> Result<u32, AppError> {
    // SAFETY: GetClipboardSequenceNumber 无参数，始终安全
    let seq = unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() };
    Ok(seq)
}

/// 打开剪贴板
///
/// 返回 RAII 守卫，drop 时自动关闭剪贴板。
/// 失败时返回错误，调用方应重试或跳过。
pub fn open_clipboard() -> Result<ClipboardGuard, AppError> {
    // SAFETY: OpenClipboard 接受 HWND 或 null，传 null 表示不指定所有者
    let result = unsafe { OpenClipboard(None) };
    if result.is_err() {
        return Err(AppError::ClipboardBusy);
    }
    Ok(ClipboardGuard)
}

/// 剪贴板 RAII 守卫
///
/// drop 时自动调用 CloseClipboard，确保剪贴板句柄不会泄漏。
pub struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: 守卫存在时剪贴板已打开，CloseClipboard 始终安全
        let _ = unsafe { CloseClipboard() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(non_snake_case)]
    fn 序列号获取成功() {
        // 此测试需要 Windows 环境
        let result = get_sequence_number();
        assert!(result.is_ok());
    }

    #[test]
    #[allow(non_snake_case)]
    fn 序列号溢出处理() {
        // 模拟序列号回绕
        let last_seq = 0xFFFFFFFE_u32;
        let current_seq = 0x00000002_u32;
        // wrapping_sub 正确处理回绕
        let diff = current_seq.wrapping_sub(last_seq);
        assert_eq!(diff, 4);
        assert_ne!(diff, 0);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 序列号无变化() {
        let last_seq = 100_u32;
        let current_seq = 100_u32;
        let diff = current_seq.wrapping_sub(last_seq);
        assert_eq!(diff, 0);
    }
}
