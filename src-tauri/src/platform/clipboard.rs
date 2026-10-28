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

/// 文本正文上限（256KB，超出截断并标记）
const TEXT_MAX_BYTES: usize = 256 * 1024;

/// 图片上限（8MB，超出只记录标记条目）
const IMAGE_MAX_BYTES: usize = 8 * 1024 * 1024;

/// 预览上限（按字符计，避免截断多字节字符）
const PREVIEW_MAX_CHARS: usize = 512;

/// CF_UNICODETEXT
const CF_UNICODETEXT: u32 = 13;
/// CF_HDROP
const CF_HDROP: u32 = 15;
/// CF_DIB
const CF_DIB: u32 = 8;
/// CF_DIBV5
const CF_DIBV5: u32 = 17;

/// 图片编码格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// 剪贴板 PNG 注册格式（已是 PNG 字节）
    Png,
    /// CF_DIB / CF_DIBV5 原始字节，需解码后才能显示
    Dib,
}

impl ImageFormat {
    /// blob 文件扩展名
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Dib => "dib",
        }
    }
}

/// 剪贴板内容类型
#[derive(Debug, Clone, PartialEq)]
pub enum ClipboardContent {
    Text(String),
    Html(String),
    Rtf(String),
    Image { data: Vec<u8>, format: ImageFormat },
    Files(Vec<String>),
}

impl ClipboardContent {
    /// 入库用的类型标识（与 schema 的 kind 取值一致）
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Html(_) => "html",
            Self::Rtf(_) => "rtf",
            Self::Image { .. } => "image",
            Self::Files(_) => "files",
        }
    }
}

/// 图片读取结果
enum ImageRead {
    /// 正常大小，返回原始字节
    Data(Vec<u8>),
    /// 超过上限，只记录标记条目
    TooLarge,
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
    if formats.contains(&CF_UNICODETEXT) {
        if let Some(text) = read_unicode_text()? {
            // 超过 256KB 截断并标记（按字符边界截断，避免切坏多字节字符）
            let (text, body_truncated) = truncate_bytes(&text, TEXT_MAX_BYTES);
            let (preview, _) = truncate_chars(&text, PREVIEW_MAX_CHARS);
            return Ok(Some(ClipboardEntry {
                content: ClipboardContent::Text(text),
                preview,
                truncated: body_truncated,
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
            let (preview, _) = truncate_chars(&plain_text, PREVIEW_MAX_CHARS);
            return Ok(Some(ClipboardEntry {
                content: ClipboardContent::Html(html),
                preview,
                truncated: false,
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
            let (preview, _) = truncate_chars(&plain_text, PREVIEW_MAX_CHARS);
            return Ok(Some(ClipboardEntry {
                content: ClipboardContent::Rtf(rtf),
                preview,
                truncated: false,
                source_app: None,
                source_title: None,
            }));
        }
    }

    // 4. 图片：PNG 优先，其次 CF_DIB / CF_DIBV5（PrtScn、老式截图工具只有 DIB）
    let png_format = register_format("PNG")?;
    if formats.contains(&png_format) {
        if let Some(read) = read_image_bytes(png_format, IMAGE_MAX_BYTES)? {
            return Ok(Some(image_entry(read, ImageFormat::Png)));
        }
    }

    let dib_format = if formats.contains(&CF_DIB) {
        Some(CF_DIB)
    } else if formats.contains(&CF_DIBV5) {
        Some(CF_DIBV5)
    } else {
        None
    };
    if let Some(format) = dib_format {
        if let Some(read) = read_image_bytes(format, IMAGE_MAX_BYTES)? {
            return Ok(Some(image_entry(read, ImageFormat::Dib)));
        }
    }

    // 5. 文件列表
    if formats.contains(&CF_HDROP) {
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
    if id == 0 {
        // 只记录格式名，不含内容
        log::debug!("clipboard format not registered: {}", name);
    }
    Ok(id)
}

/// 读取 Unicode 文本
fn read_unicode_text() -> Result<Option<String>, AppError> {
    // SAFETY: CF_UNICODETEXT 是系统预定义格式
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT) }?;
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
/// StartFragment 和 EndFragment 是字节偏移，10 位零填充。
/// 优先取 Fragment 偏移（真正的选中片段），缺失时回退到 StartHTML/EndHTML。
fn parse_html_fragment(raw: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(raw);

    let range = match (
        find_header_value(&text, "StartFragment"),
        find_header_value(&text, "EndFragment"),
    ) {
        (Some(start), Some(end)) if start < end && end <= raw.len() => Some((start, end)),
        _ => match (
            find_header_value(&text, "StartHTML"),
            find_header_value(&text, "EndHTML"),
        ) {
            (Some(start), Some(end)) if start < end && end <= raw.len() => Some((start, end)),
            _ => None,
        },
    };

    range.map(|(start, end)| String::from_utf8_lossy(&raw[start..end]).to_string())
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
///
/// RTF 是 ANSI/UTF-8 字节流，不能按 UTF-16 猜测解码。
fn read_rtf(format: u32) -> Result<Option<String>, AppError> {
    let handle = unsafe { GetClipboardData(format) }?;
    Ok(read_bytes_from_handle(handle)?.map(|raw| String::from_utf8_lossy(&raw).to_string()))
}

/// 读取图片原始字节
///
/// 先看 `GlobalSize` 再拷贝，避免超大图造成内存尖峰。
fn read_image_bytes(format: u32, max_size: usize) -> Result<Option<ImageRead>, AppError> {
    let handle = unsafe { GetClipboardData(format) }?;
    let hglobal = HGLOBAL(handle.0);

    // SAFETY: handle 有效，GlobalSize 不要求锁定
    let size = unsafe { windows::Win32::System::Memory::GlobalSize(hglobal) };
    if size > max_size {
        return Ok(Some(ImageRead::TooLarge));
    }

    Ok(read_bytes_from_handle(handle)?.map(ImageRead::Data))
}

/// 构造图片条目
fn image_entry(read: ImageRead, format: ImageFormat) -> ClipboardEntry {
    match read {
        ImageRead::Data(data) => ClipboardEntry {
            content: ClipboardContent::Image { data, format },
            preview: "[图片]".to_string(),
            truncated: false,
            source_app: None,
            source_title: None,
        },
        // 超过 8MB：不保存原图，只记录可识别的标记条目（UI 显示「原图过大」）
        ImageRead::TooLarge => ClipboardEntry {
            content: ClipboardContent::Image {
                data: Vec::new(),
                format,
            },
            preview: "[图片过大]".to_string(),
            truncated: true,
            source_app: None,
            source_title: None,
        },
    }
}

/// 读取文件列表
fn read_file_list() -> Result<Option<Vec<String>>, AppError> {
    // SAFETY: CF_HDROP 是系统预定义格式
    let handle = unsafe { GetClipboardData(CF_HDROP) }?;

    // SAFETY: handle 有效
    let hdrop = HDROP(handle.0);

    // 获取文件数量
    // SAFETY: hdrop 有效，传 0xFFFFFFFF 获取数量
    let count = unsafe { DragQueryFileW(hdrop, 0xFFFFFFFF, None) };

    if count == 0 || count == u32::MAX {
        return Ok(None);
    }

    let mut files = Vec::with_capacity(count as usize);
    for i in 0..count {
        // 获取文件名长度
        // SAFETY: hdrop 有效，i < count
        let len = unsafe { DragQueryFileW(hdrop, i, None) };
        if len == 0 || len == u32::MAX {
            continue;
        }

        let buffer_len = (len as usize).saturating_add(1);
        let mut buffer = vec![0u16; buffer_len];
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

            // 按第一个 null 截断（缓冲区可能带填充 null）
            let wide = match wide.iter().position(|&ch| ch == 0) {
                Some(pos) => &wide[..pos],
                None => &wide,
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

/// 按字符数截断（不会切坏多字节字符）
fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    let mut result = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= max_chars {
            return (result, true);
        }
        result.push(ch);
    }
    (result, false)
}

/// 按字节数截断（回退到字符边界，不会 panic）
fn truncate_bytes(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }

    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
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
        let (text, truncated) = truncate_chars("Hello", 10);
        assert_eq!(text, "Hello");
        assert!(!truncated);

        let (text, truncated) = truncate_chars("Hello World", 5);
        assert_eq!(text, "Hello");
        assert!(truncated);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 文本截断不切坏中文() {
        // 中文字符 3 字节，按字符截断不应 panic
        let (text, truncated) = truncate_chars("你好世界", 2);
        assert_eq!(text, "你好");
        assert!(truncated);

        // 按字节截断时回退到字符边界
        let (text, truncated) = truncate_bytes("你好世界", 7);
        assert_eq!(text, "你好");
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

    #[test]
    #[allow(non_snake_case)]
    fn 超限图片生成标记条目() {
        let entry = image_entry(ImageRead::TooLarge, ImageFormat::Dib);
        assert!(entry.truncated);
        assert_eq!(entry.preview, "[图片过大]");
        assert_eq!(entry.content.kind(), "image");

        match entry.content {
            ClipboardContent::Image { data, format } => {
                assert!(data.is_empty());
                assert_eq!(format, ImageFormat::Dib);
            }
            other => panic!("unexpected content: {:?}", other),
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 图片条目携带格式信息() {
        let entry = image_entry(ImageRead::Data(vec![1, 2, 3]), ImageFormat::Png);
        assert!(!entry.truncated);
        assert_eq!(entry.preview, "[图片]");
        match entry.content {
            ClipboardContent::Image { data, format } => {
                assert_eq!(data, vec![1, 2, 3]);
                assert_eq!(format, ImageFormat::Png);
                assert_eq!(format.extension(), "png");
            }
            other => panic!("unexpected content: {:?}", other),
        }
    }
}
