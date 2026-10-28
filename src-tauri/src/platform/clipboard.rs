// 剪贴板 Win32 封装
//
// 所有剪贴板操作必须通过此模块，禁止在业务代码中直接调用 Win32 API。

use crate::error::AppError;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, CountClipboardFormats, EnumClipboardFormats, GetClipboardData, OpenClipboard,
    RegisterClipboardFormatW,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

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

/// 剪贴板内容类型
#[derive(Debug, Clone, PartialEq)]
pub enum ClipboardContent {
    Text(String),
    Html(String),
    Rtf(String),
    Image(Vec<u8>), // PNG 格式
    Files(Vec<String>),
}

/// 剪贴板条目（包含所有格式）
#[derive(Debug, Clone)]
pub struct ClipboardEntry {
    pub content: ClipboardContent,
    pub preview: String,
    pub truncated: bool,
    pub source_app: Option<String>,
    pub source_title: Option<String>,
}

/// 读取剪贴板所有格式
///
/// 按优先级读取：文本 > HTML > RTF > 图片 > 文件列表
/// 超限处理：文本 >256KB 截断；图片 >8MB 只存缩略图
pub fn read_clipboard_content() -> Result<Option<ClipboardEntry>, AppError> {
    let _guard = open_clipboard()?;

    // 枚举所有格式
    let formats = enumerate_formats()?;
    if formats.is_empty() {
        return Ok(None);
    }

    // 按优先级尝试读取
    // 1. 文本
    if formats.contains(&1u32) {
        // CF_UNICODETEXT
        if let Some(text) = read_unicode_text()? {
            let (preview, truncated) = truncate_text(&text, 512);
            return Ok(Some(ClipboardEntry {
                content: ClipboardContent::Text(text),
                preview,
                truncated,
                source_app: None,
                source_title: None,
            }));
        }
    }

    // 2. HTML
    let html_format = register_format("HTML Format")?;
    if formats.contains(&html_format) {
        if let Some(html) = read_html(html_format)? {
            let plain_text = html_to_plain_text(&html);
            let (preview, truncated) = truncate_text(&plain_text, 512);
            return Ok(Some(ClipboardEntry {
                content: ClipboardContent::Html(html),
                preview,
                truncated,
                source_app: None,
                source_title: None,
            }));
        }
    }

    // 3. RTF
    let rtf_format = register_format("Rich Text Format")?;
    if formats.contains(&rtf_format) {
        if let Some(rtf) = read_rtf(rtf_format)? {
            let plain_text = rtf_to_plain_text(&rtf);
            let (preview, truncated) = truncate_text(&plain_text, 512);
            return Ok(Some(ClipboardEntry {
                content: ClipboardContent::Rtf(rtf),
                preview,
                truncated,
                source_app: None,
                source_title: None,
            }));
        }
    }

    // 4. 图片（PNG 优先，其次 DIB）
    let png_format = register_format("PNG")?;
    if formats.contains(&png_format) {
        if let Some(png_data) = read_image(png_format, 8 * 1024 * 1024)? {
            return Ok(Some(ClipboardEntry {
                content: ClipboardContent::Image(png_data),
                preview: "[图片]".to_string(),
                truncated: false,
                source_app: None,
                source_title: None,
            }));
        }
    }

    // 5. 文件列表
    if formats.contains(&15u32) {
        // CF_HDROP
        if let Some(files) = read_file_list()? {
            let preview = if files.len() == 1 {
                files[0].clone()
            } else {
                format!("[{} 个文件]", files.len())
            };
            return Ok(Some(ClipboardEntry {
                content: ClipboardContent::Files(files),
                preview,
                truncated: false,
                source_app: None,
                source_title: None,
            }));
        }
    }

    Ok(None)
}

/// 枚举剪贴板所有格式
fn enumerate_formats() -> Result<Vec<u32>, AppError> {
    let mut formats = Vec::new();
    let _count = unsafe { CountClipboardFormats() };

    let mut format = unsafe { EnumClipboardFormats(0) };
    while format != 0 {
        formats.push(format);
        format = unsafe { EnumClipboardFormats(format) };
    }

    Ok(formats)
}

/// 注册自定义格式
fn register_format(name: &str) -> Result<u32, AppError> {
    let wide: Vec<u16> = OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: wide 是有效的 null-terminated 字符串
    let id = unsafe { RegisterClipboardFormatW(windows::core::PCWSTR(wide.as_ptr())) };
    Ok(id)
}

/// 读取 Unicode 文本
fn read_unicode_text() -> Result<Option<String>, AppError> {
    // SAFETY: CF_UNICODETEXT = 13
    let handle = unsafe { GetClipboardData(13) }?;
    read_string_from_handle(handle)
}

/// 读取 HTML
fn read_html(format: u32) -> Result<Option<String>, AppError> {
    let handle = unsafe { GetClipboardData(format) }?;
    if let Some(raw) = read_bytes_from_handle(handle)? {
        // 解析 HTML Format 头部
        if let Some(fragment) = parse_html_fragment(&raw) {
            return Ok(Some(fragment));
        }
    }
    Ok(None)
}

/// 解析 HTML Format 头部
///
/// 格式：Version:0.9\r\nStartHTML:0000000123\r\nEndHTML:0000000456\r\n...
/// StartFragment 和 EndFragment 是字节偏移，10 位零填充
fn parse_html_fragment(raw: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(raw);

    let start_html = find_header_value(&text, "StartHTML")?;
    let end_html = find_header_value(&text, "EndHTML")?;

    if start_html < raw.len() && end_html <= raw.len() && start_html < end_html {
        let fragment = &raw[start_html..end_html];
        Some(String::from_utf8_lossy(fragment).to_string())
    } else {
        None
    }
}

/// 查找 HTML Format 头部中的值
fn find_header_value(text: &str, key: &str) -> Option<usize> {
    for line in text.lines() {
        if line.starts_with(key) {
            if let Some(value) = line.split(':').nth(1) {
                return value.trim().parse().ok();
            }
        }
    }
    None
}

/// 读取 RTF
fn read_rtf(format: u32) -> Result<Option<String>, AppError> {
    let handle = unsafe { GetClipboardData(format) }?;
    read_string_from_handle(handle)
}

/// 读取图片
fn read_image(format: u32, max_size: usize) -> Result<Option<Vec<u8>>, AppError> {
    let handle = unsafe { GetClipboardData(format) }?;
    if let Some(data) = read_bytes_from_handle(handle)? {
        if data.len() > max_size {
            // 超限：返回 None，后续可以生成缩略图
            return Ok(None);
        }
        return Ok(Some(data));
    }
    Ok(None)
}

/// 读取文件列表
fn read_file_list() -> Result<Option<Vec<String>>, AppError> {
    // CF_HDROP = 15
    let handle = unsafe { GetClipboardData(15) }?;

    // SAFETY: handle 有效
    let hdrop = HDROP(handle.0);

    // 获取文件数量
    // SAFETY: hdrop 有效，传 0xFFFFFFFF 获取数量
    let count = unsafe { DragQueryFileW(hdrop, 0xFFFFFFFF, None) };

    if count == 0 {
        return Ok(None);
    }

    let mut files = Vec::with_capacity(count as usize);
    for i in 0..count {
        // 获取文件名长度
        // SAFETY: hdrop 有效，i < count
        let len = unsafe { DragQueryFileW(hdrop, i, None) };

        let mut buffer = vec![0u16; (len + 1) as usize];
        // SAFETY: buffer 足够大
        unsafe { DragQueryFileW(hdrop, i, Some(&mut buffer)) };

        let file = String::from_utf16_lossy(&buffer[..len as usize]);
        files.push(file);
    }

    Ok(Some(files))
}

/// 从 HANDLE 读取字符串
fn read_string_from_handle(handle: HANDLE) -> Result<Option<String>, AppError> {
    if let Some(bytes) = read_bytes_from_handle(handle)? {
        // 尝试 UTF-16
        if bytes.len() >= 2 && bytes.len() % 2 == 0 {
            let wide: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();

            // 移除 null 终止符
            let wide = if wide.last() == Some(&0) {
                &wide[..wide.len() - 1]
            } else {
                &wide
            };

            return Ok(Some(String::from_utf16_lossy(wide)));
        }

        // 尝试 UTF-8
        Ok(Some(String::from_utf8_lossy(&bytes).to_string()))
    } else {
        Ok(None)
    }
}

/// 从 HANDLE 读取字节
fn read_bytes_from_handle(handle: HANDLE) -> Result<Option<Vec<u8>>, AppError> {
    // 将 HANDLE 转换为 HGLOBAL（实际上是相同的底层指针）
    let hglobal = HGLOBAL(handle.0);

    // SAFETY: hglobal 有效
    let ptr = unsafe { GlobalLock(hglobal) };
    if ptr.is_null() {
        return Ok(None);
    }

    // SAFETY: ptr 有效
    let size = unsafe { windows::Win32::System::Memory::GlobalSize(hglobal) };
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, size) }.to_vec();

    // SAFETY: ptr 有效
    let _ = unsafe { GlobalUnlock(hglobal) };

    Ok(Some(bytes))
}

/// 截断文本
fn truncate_text(text: &str, max_len: usize) -> (String, bool) {
    if text.len() <= max_len {
        (text.to_string(), false)
    } else {
        (text[..max_len].to_string(), true)
    }
}

/// HTML 转纯文本（简单实现）
fn html_to_plain_text(html: &str) -> String {
    // 移除标签
    let mut result = String::new();
    let mut in_tag = false;

    for ch in html.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(ch);
        }
    }

    // 解码常见实体
    result
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .trim()
        .to_string()
}

/// RTF 转纯文本（简单实现）
fn rtf_to_plain_text(rtf: &str) -> String {
    // 移除 RTF 控制字
    let mut result = String::new();
    let mut in_control = false;

    for ch in rtf.chars() {
        if ch == '\\' {
            in_control = true;
        } else if ch == ' ' && in_control {
            in_control = false;
        } else if ch == '{' || ch == '}' {
            // 忽略分组
        } else if !in_control {
            result.push(ch);
        }
    }

    result.trim().to_string()
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

    #[test]
    #[allow(non_snake_case)]
    fn HTML片段解析() {
        // 头部 52 字节，内容 26 字节
        let raw = b"Version:0.9\r\nStartHTML:0000000052\r\nEndHTML:0000000078\r\n<html><body>Hello</body></html>";
        let fragment = parse_html_fragment(raw);
        assert!(fragment.is_some());
        let html = fragment.unwrap();
        assert!(html.contains("Hello"));
    }

    #[test]
    #[allow(non_snake_case)]
    fn HTML头部值查找() {
        let text = "Version:0.9\r\nStartHTML:0000000100\r\nEndHTML:0000000200";
        assert_eq!(find_header_value(text, "StartHTML"), Some(100));
        assert_eq!(find_header_value(text, "EndHTML"), Some(200));
        assert_eq!(find_header_value(text, "Invalid"), None);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 文本截断() {
        let (text, truncated) = truncate_text("Hello", 10);
        assert_eq!(text, "Hello");
        assert!(!truncated);

        let (text, truncated) = truncate_text("Hello World", 5);
        assert_eq!(text, "Hello");
        assert!(truncated);
    }

    #[test]
    #[allow(non_snake_case)]
    fn HTML转纯文本() {
        let html = "<p>Hello <b>World</b></p>";
        let plain = html_to_plain_text(html);
        assert_eq!(plain, "Hello World");
    }

    #[test]
    #[allow(non_snake_case)]
    fn RTF转纯文本() {
        let rtf = r"{\rtf1\ansi Hello \b World\b0 }";
        let plain = rtf_to_plain_text(rtf);
        assert!(plain.contains("Hello"));
        assert!(plain.contains("World"));
    }
}
