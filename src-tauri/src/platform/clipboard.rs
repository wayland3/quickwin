// 剪贴板平台封装
//
// Windows: Win32 API 实现
// Linux: arboard 库实现（X11/Wayland）

use crate::error::AppError;

/// 文本正文上限（256KB，超出截断并标记）
const TEXT_MAX_BYTES: usize = 256 * 1024;

/// 图片上限（8MB，超出只记录标记条目）
const IMAGE_MAX_BYTES: usize = 8 * 1024 * 1024;

/// 预览上限（按字符计，避免截断多字节字符）
const PREVIEW_MAX_CHARS: usize = 512;

/// 图片编码格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// 剪贴板 PNG 注册格式（已是 PNG 字节）
    Png,
    /// CF_DIB / CF_DIBV5 原始字节，需解码后才能显示
    #[cfg(target_os = "windows")]
    Dib,
    /// 原始 RGBA 字节（Linux）
    #[cfg(target_os = "linux")]
    Raw,
}

impl ImageFormat {
    /// blob 文件扩展名
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Png => "png",
            #[cfg(target_os = "windows")]
            Self::Dib => "dib",
            #[cfg(target_os = "linux")]
            Self::Raw => "raw",
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

/// 剪贴板条目（包含所有格式）
#[derive(Debug, Clone)]
pub struct ClipboardEntry {
    pub content: ClipboardContent,
    pub preview: String,
    pub truncated: bool,
    pub source_app: Option<String>,
    pub source_title: Option<String>,
}

/// 获取剪贴板序列号
///
/// 用于检测剪贴板变化。每次剪贴板内容改变，序列号都会递增。
/// 序列号为 u32，会在 u32::MAX 后回绕到 0，使用 wrapping_sub 处理。
pub fn get_sequence_number() -> Result<u32, AppError> {
    #[cfg(target_os = "windows")]
    {
        get_sequence_number_windows()
    }

    #[cfg(target_os = "linux")]
    {
        get_sequence_number_linux()
    }
}

/// 读取剪贴板所有格式
///
/// 按优先级读取：文本 > HTML > RTF > 图片 > 文件列表
/// 超限处理：文本 >256KB 截断；图片 >8MB 只存缩略图
pub fn read_clipboard_content() -> Result<Option<ClipboardEntry>, AppError> {
    #[cfg(target_os = "windows")]
    {
        read_clipboard_content_windows()
    }

    #[cfg(target_os = "linux")]
    {
        read_clipboard_content_linux()
    }
}

/// 写入剪贴板内容
///
/// 按格式写入：文本 + HTML + RTF + 图片 + 文件列表
/// 任一格式失败不影响其他格式
pub fn write_clipboard_content(content: &ClipboardContent) -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        write_clipboard_content_windows(content)
    }

    #[cfg(target_os = "linux")]
    {
        write_clipboard_content_linux(content)
    }
}

/// 按字节截断（不切坏 UTF-8）
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

/// 按字符截断
fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    let mut result = String::new();
    for (i, ch) in text.chars().enumerate() {
        if i >= max_chars {
            return (result, true);
        }
        result.push(ch);
    }
    (result, false)
}

// ============================================================
// Windows 实现
// ============================================================
#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, CountClipboardFormats, EmptyClipboard, EnumClipboardFormats,
        GetClipboardData, GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW,
        SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

    const CF_UNICODETEXT: u32 = 13;
    const CF_HDROP: u32 = 15;
    const CF_DIB: u32 = 8;
    const CF_DIBV5: u32 = 17;

    pub struct ClipboardGuard;

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            let _ = unsafe { CloseClipboard() };
        }
    }

    fn open_clipboard() -> Result<ClipboardGuard, AppError> {
        let result = unsafe { OpenClipboard(None) };
        if result.is_err() {
            return Err(AppError::ClipboardBusy);
        }
        Ok(ClipboardGuard)
    }

    pub fn get_sequence_number_windows() -> Result<u32, AppError> {
        let seq = unsafe { GetClipboardSequenceNumber() };
        Ok(seq)
    }

    pub fn read_clipboard_content_windows() -> Result<Option<ClipboardEntry>, AppError> {
        let _guard = open_clipboard()?;

        let formats = enumerate_formats()?;
        if formats.is_empty() {
            return Ok(None);
        }

        // 1. 文本
        if formats.contains(&CF_UNICODETEXT) {
            if let Some(text) = read_unicode_text()? {
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
        if let Some(html_format) = register_format("HTML Format") {
            if formats.contains(&html_format) {
                if let Some(html) = read_html(html_format)? {
                    let preview = html_to_preview(&html);
                    return Ok(Some(ClipboardEntry {
                        content: ClipboardContent::Html(html),
                        preview,
                        truncated: false,
                        source_app: None,
                        source_title: None,
                    }));
                }
            }
        }

        // 3. RTF
        if let Some(rtf_format) = register_format("Rich Text Format") {
            if formats.contains(&rtf_format) {
                if let Some(rtf) = read_rtf(rtf_format)? {
                    let preview = rtf_to_preview(&rtf);
                    return Ok(Some(ClipboardEntry {
                        content: ClipboardContent::Rtf(rtf),
                        preview,
                        truncated: false,
                        source_app: None,
                        source_title: None,
                    }));
                }
            }
        }

        // 4. 图片（PNG 优先，然后 DIB/DIBV5）
        if let Some(png_format) = register_format("PNG") {
            if formats.contains(&png_format) {
                if let Some(image_read) = read_image_data(png_format)? {
                    return match image_read {
                        ImageRead::Data(data) => Ok(Some(ClipboardEntry {
                            content: ClipboardContent::Image {
                                data,
                                format: ImageFormat::Png,
                            },
                            preview: "[图片]".to_string(),
                            truncated: false,
                            source_app: None,
                            source_title: None,
                        })),
                        ImageRead::TooLarge => Ok(Some(ClipboardEntry {
                            content: ClipboardContent::Image {
                                data: Vec::new(),
                                format: ImageFormat::Png,
                            },
                            preview: "[图片过大]".to_string(),
                            truncated: true,
                            source_app: None,
                            source_title: None,
                        })),
                    };
                }
            }
        }

        for fmt in [CF_DIB, CF_DIBV5] {
            if formats.contains(&fmt) {
                if let Some(image_read) = read_image_data(fmt)? {
                    return match image_read {
                        ImageRead::Data(data) => Ok(Some(ClipboardEntry {
                            content: ClipboardContent::Image {
                                data,
                                format: ImageFormat::Dib,
                            },
                            preview: "[图片]".to_string(),
                            truncated: false,
                            source_app: None,
                            source_title: None,
                        })),
                        ImageRead::TooLarge => Ok(Some(ClipboardEntry {
                            content: ClipboardContent::Image {
                                data: Vec::new(),
                                format: ImageFormat::Dib,
                            },
                            preview: "[图片过大]".to_string(),
                            truncated: true,
                            source_app: None,
                            source_title: None,
                        })),
                    };
                }
            }
        }

        // 5. 文件列表
        if formats.contains(&CF_HDROP) {
            if let Some(files) = read_file_list()? {
                let preview = files_to_preview(&files);
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

    fn enumerate_formats() -> Result<Vec<u32>, AppError> {
        let count = unsafe { CountClipboardFormats() };
        let mut formats = Vec::with_capacity(count as usize);

        let mut fmt = 0u32;
        loop {
            let next = unsafe { EnumClipboardFormats(fmt) };
            if next == 0 {
                break;
            }
            formats.push(next);
            fmt = next;
        }

        Ok(formats)
    }

    fn register_format(name: &str) -> Option<u32> {
        let wide: Vec<u16> = OsStr::new(name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let fmt = unsafe { RegisterClipboardFormatW(windows::core::PCWSTR(wide.as_ptr())) };
        if fmt == 0 {
            None
        } else {
            Some(fmt)
        }
    }

    fn read_unicode_text() -> Result<Option<String>, AppError> {
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT) }.ok()?;
        let ptr = unsafe { GlobalLock(handle) } as *const u16;
        if ptr.is_null() {
            return Ok(None);
        }

        let mut len = 0;
        while unsafe { *ptr.add(len) } != 0 {
            len += 1;
        }

        let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) });
        unsafe { GlobalUnlock(handle) };

        Ok(Some(text))
    }

    fn read_html(format: u32) -> Result<Option<String>, AppError> {
        let handle = unsafe { GetClipboardData(format) }.ok()?;
        let ptr = unsafe { GlobalLock(handle) } as *const u8;
        if ptr.is_null() {
            return Ok(None);
        }

        let size = unsafe { GlobalSize(handle) };
        let bytes = unsafe { std::slice::from_raw_parts(ptr, size) };
        let html = String::from_utf8_lossy(bytes).to_string();
        unsafe { GlobalUnlock(handle) };

        Ok(Some(html))
    }

    fn read_rtf(format: u32) -> Result<Option<String>, AppError> {
        let handle = unsafe { GetClipboardData(format) }.ok()?;
        let ptr = unsafe { GlobalLock(handle) } as *const u8;
        if ptr.is_null() {
            return Ok(None);
        }

        let size = unsafe { GlobalSize(handle) };
        let bytes = unsafe { std::slice::from_raw_parts(ptr, size) };
        let rtf = String::from_utf8_lossy(bytes).to_string();
        unsafe { GlobalUnlock(handle) };

        Ok(Some(rtf))
    }

    enum ImageRead {
        Data(Vec<u8>),
        TooLarge,
    }

    fn read_image_data(format: u32) -> Result<Option<ImageRead>, AppError> {
        let handle = unsafe { GetClipboardData(format) }.ok()?;
        let ptr = unsafe { GlobalLock(handle) } as *const u8;
        if ptr.is_null() {
            return Ok(None);
        }

        let size = unsafe { GlobalSize(handle) };
        if size > IMAGE_MAX_BYTES {
            unsafe { GlobalUnlock(handle) };
            return Ok(Some(ImageRead::TooLarge));
        }

        let bytes = unsafe { std::slice::from_raw_parts(ptr, size) };
        let data = bytes.to_vec();
        unsafe { GlobalUnlock(handle) };

        Ok(Some(ImageRead::Data(data)))
    }

    fn read_file_list() -> Result<Option<Vec<String>>, AppError> {
        let handle = unsafe { GetClipboardData(CF_HDROP) }.ok()?;
        let hdrop = HDROP(handle.0);

        let count = unsafe { DragQueryFileW(hdrop, 0xFFFFFFFF, None) };
        if count == 0 {
            return Ok(None);
        }

        let mut files = Vec::with_capacity(count as usize);
        for i in 0..count {
            let len = unsafe { DragQueryFileW(hdrop, i, None) };
            let mut buf = vec![0u16; (len + 1) as usize];
            unsafe { DragQueryFileW(hdrop, i, Some(&mut buf)) };
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            files.push(path);
        }

        Ok(Some(files))
    }

    fn html_to_preview(html: &str) -> String {
        // 简化版：去掉 HTML 标签
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
            if result.len() >= PREVIEW_MAX_CHARS {
                break;
            }
        }
        result
    }

    fn rtf_to_preview(rtf: &str) -> String {
        // 简化版：去掉 RTF 控制字
        let mut result = String::new();
        let mut in_control = false;
        for ch in rtf.chars() {
            if ch == '\\' {
                in_control = true;
            } else if ch == ' ' && in_control {
                in_control = false;
            } else if !in_control && ch != '{' && ch != '}' {
                result.push(ch);
            }
            if result.len() >= PREVIEW_MAX_CHARS {
                break;
            }
        }
        result
    }

    fn files_to_preview(files: &[String]) -> String {
        if files.is_empty() {
            return String::new();
        }
        if files.len() == 1 {
            return files[0].clone();
        }
        format!("{} 个文件", files.len())
    }

    pub fn write_clipboard_content_windows(content: &ClipboardContent) -> Result<(), AppError> {
        let _guard = open_clipboard()?;
        unsafe { EmptyClipboard() }?;

        match content {
            ClipboardContent::Text(text) => {
                write_unicode_text(text)?;
            }
            ClipboardContent::Html(html) => {
                write_unicode_text(&html_to_text(html))?;
                write_html(html)?;
            }
            ClipboardContent::Rtf(rtf) => {
                write_unicode_text(&rtf_to_text(rtf))?;
                write_rtf(rtf)?;
            }
            ClipboardContent::Image { data, format } => {
                write_image(data, *format)?;
            }
            ClipboardContent::Files(files) => {
                write_file_list(files)?;
            }
        }

        Ok(())
    }

    fn write_unicode_text(text: &str) -> Result<(), AppError> {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let size = wide.len() * 2;
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, size) }?;

        let ptr = unsafe { GlobalLock(handle) } as *mut u16;
        if ptr.is_null() {
            unsafe { GlobalFree(handle) };
            return Err(AppError::Internal("GlobalLock failed".into()));
        }

        unsafe { std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len()) };
        unsafe { GlobalUnlock(handle) };

        unsafe { SetClipboardData(CF_UNICODETEXT, HANDLE(handle.0 as *mut _)) }?;

        Ok(())
    }

    fn write_html(html: &str) -> Result<(), AppError> {
        if let Some(format) = register_format("HTML Format") {
            let bytes = html.as_bytes();
            let size = bytes.len();
            let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, size) }?;

            let ptr = unsafe { GlobalLock(handle) } as *mut u8;
            if ptr.is_null() {
                unsafe { GlobalFree(handle) };
                return Err(AppError::Internal("GlobalLock failed".into()));
            }

            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, size) };
            unsafe { GlobalUnlock(handle) };

            unsafe { SetClipboardData(format, HANDLE(handle.0 as *mut _)) }?;
        }

        Ok(())
    }

    fn write_rtf(rtf: &str) -> Result<(), AppError> {
        if let Some(format) = register_format("Rich Text Format") {
            let bytes = rtf.as_bytes();
            let size = bytes.len();
            let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, size) }?;

            let ptr = unsafe { GlobalLock(handle) } as *mut u8;
            if ptr.is_null() {
                unsafe { GlobalFree(handle) };
                return Err(AppError::Internal("GlobalLock failed".into()));
            }

            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, size) };
            unsafe { GlobalUnlock(handle) };

            unsafe { SetClipboardData(format, HANDLE(handle.0 as *mut _)) }?;
        }

        Ok(())
    }

    fn write_image(data: &[u8], format: ImageFormat) -> Result<(), AppError> {
        let size = data.len();
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, size) }?;

        let ptr = unsafe { GlobalLock(handle) } as *mut u8;
        if ptr.is_null() {
            unsafe { GlobalFree(handle) };
            return Err(AppError::Internal("GlobalLock failed".into()));
        }

        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, size) };
        unsafe { GlobalUnlock(handle) };

        let cf = match format {
            ImageFormat::Png => register_format("PNG").unwrap_or(0),
            ImageFormat::Dib => CF_DIB,
        };

        if cf != 0 {
            unsafe { SetClipboardData(cf, HANDLE(handle.0 as *mut _)) }?;
        }

        Ok(())
    }

    fn write_file_list(files: &[String]) -> Result<(), AppError> {
        // 简化版：暂不实现
        Ok(())
    }

    fn html_to_text(html: &str) -> String {
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
        result
    }

    fn rtf_to_text(rtf: &str) -> String {
        let mut result = String::new();
        let mut in_control = false;
        for ch in rtf.chars() {
            if ch == '\\' {
                in_control = true;
            } else if ch == ' ' && in_control {
                in_control = false;
            } else if !in_control && ch != '{' && ch != '}' {
                result.push(ch);
            }
        }
        result
    }
}

#[cfg(target_os = "windows")]
use windows_impl::*;

// ============================================================
// Linux 实现
// ============================================================
#[cfg(target_os = "linux")]
mod linux_impl {
    use super::*;
    use arboard::Clipboard;
    use sha2::{Digest, Sha256};
    use std::sync::Mutex;

    static CLIPBOARD: Mutex<Option<Clipboard>> = Mutex::new(None);
    static LAST_HASH: Mutex<Option<String>> = Mutex::new(None);

    fn get_clipboard() -> Result<std::sync::MutexGuard<'static, Option<Clipboard>>, AppError> {
        let mut guard = CLIPBOARD
            .lock()
            .map_err(|_| AppError::Internal("clipboard mutex poisoned".into()))?;

        if guard.is_none() {
            *guard = Some(
                Clipboard::new()
                    .map_err(|e| AppError::Internal(format!("clipboard init failed: {}", e)))?,
            );
        }

        Ok(guard)
    }

    pub fn get_sequence_number_linux() -> Result<u32, AppError> {
        let mut clipboard = get_clipboard()?;
        let cb = clipboard.as_mut().ok_or(AppError::ClipboardRead)?;

        let content_key = match cb.get_text() {
            Ok(text) => text,
            Err(_) => match cb.get_image() {
                Ok(img) => format!("image:{}x{}", img.width, img.height),
                Err(_) => String::new(),
            },
        };

        let hash = compute_hash(&content_key);

        let mut last = LAST_HASH
            .lock()
            .map_err(|_| AppError::Internal("hash mutex poisoned".into()))?;
        *last = Some(hash.clone());

        Ok(hash_to_u32(&hash))
    }

    fn compute_hash(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn hash_to_u32(hash: &str) -> u32 {
        let bytes = hash.as_bytes();
        let mut result = 0u32;
        for byte in bytes.iter().take(4) {
            result = (result << 8) | (*byte as u32 & 0xFF);
        }
        result
    }

    pub fn read_clipboard_content_linux() -> Result<Option<ClipboardEntry>, AppError> {
        let mut clipboard = get_clipboard()?;
        let cb = clipboard.as_mut().ok_or(AppError::ClipboardRead)?;

        // 优先读文本
        if let Ok(text) = cb.get_text() {
            if !text.is_empty() {
                let (text, truncated) = truncate_bytes(&text, TEXT_MAX_BYTES);
                let (preview, _) = truncate_chars(&text, PREVIEW_MAX_CHARS);
                return Ok(Some(ClipboardEntry {
                    content: ClipboardContent::Text(text),
                    preview,
                    truncated,
                    source_app: None,
                    source_title: None,
                }));
            }
        }

        // 尝试读图片
        if let Ok(img) = cb.get_image() {
            let bytes = img.bytes.into_owned();
            if bytes.len() > IMAGE_MAX_BYTES {
                return Ok(Some(ClipboardEntry {
                    content: ClipboardContent::Image {
                        data: Vec::new(),
                        format: ImageFormat::Raw,
                    },
                    preview: "[图片过大]".to_string(),
                    truncated: true,
                    source_app: None,
                    source_title: None,
                }));
            }

            // 转换为 PNG
            let png_data = rgba_to_png(img.width, img.height, &bytes)?;
            let preview = "[图片]".to_string();
            return Ok(Some(ClipboardEntry {
                content: ClipboardContent::Image {
                    data: png_data,
                    format: ImageFormat::Png,
                },
                preview,
                truncated: false,
                source_app: None,
                source_title: None,
            }));
        }

        Ok(None)
    }

    fn rgba_to_png(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>, AppError> {
        use image::{codecs::png::PngEncoder, ImageBuffer, RgbaImage};
        use std::io::Cursor;

        let buffer: RgbaImage =
            ImageBuffer::from_raw(width as u32, height as u32, rgba.to_vec())
                .ok_or_else(|| AppError::Internal("invalid image dimensions".into()))?;

        let mut png_bytes = Vec::new();
        let cursor = Cursor::new(&mut png_bytes);
        let encoder = PngEncoder::new(cursor);

        use image::ImageEncoder;
        encoder
            .write_image(
                buffer.as_raw(),
                width as u32,
                height as u32,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| AppError::Internal(format!("PNG encode failed: {}", e)))?;

        Ok(png_bytes)
    }

    pub fn write_clipboard_content_linux(content: &ClipboardContent) -> Result<(), AppError> {
        let mut clipboard = get_clipboard()?;
        let cb = clipboard.as_mut().ok_or(AppError::ClipboardRead)?;

        match content {
            ClipboardContent::Text(text)
            | ClipboardContent::Html(text)
            | ClipboardContent::Rtf(text) => {
                cb.set_text(text.clone())
                    .map_err(|e| AppError::Internal(format!("clipboard write failed: {}", e)))?;
            }
            ClipboardContent::Image { data, .. } => {
                if data.is_empty() {
                    return Err(AppError::ClipboardRead);
                }
                // 解码 PNG 为 RGBA
                let img = image::load_from_memory(data)
                    .map_err(|e| AppError::Internal(format!("image decode failed: {}", e)))?;
                let rgba = img.to_rgba8();
                let (width, height) = rgba.dimensions();

                let img_data = arboard::ImageData {
                    width: width as usize,
                    height: height as usize,
                    bytes: rgba.into_raw().into(),
                };
                cb.set_image(img_data).map_err(|e| {
                    AppError::Internal(format!("clipboard image write failed: {}", e))
                })?;
            }
            ClipboardContent::Files(files) => {
                // Linux 剪贴板文件列表用 URI 列表
                let uris: Vec<String> = files.iter().map(|f| format!("file://{}", f)).collect();
                cb.set_text(uris.join("\n"))
                    .map_err(|e| AppError::Internal(format!("clipboard write failed: {}", e)))?;
            }
        }

        Ok(())
    }
}

#[cfg(target_os = "linux")]
use linux_impl::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 文本截断按字符() {
        let (text, truncated) = truncate_chars("你好世界", 2);
        assert_eq!(text, "你好");
        assert!(truncated);
    }

    #[test]
    fn 文本截断按字节不切坏() {
        let (text, truncated) = truncate_bytes("你好世界", 7);
        assert_eq!(text, "你好");
        assert!(truncated);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn 序列号生成() {
        // 测试序列号生成不会崩溃
        let result = get_sequence_number();
        assert!(result.is_ok());
    }
}
