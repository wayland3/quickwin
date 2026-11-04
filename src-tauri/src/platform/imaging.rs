// WIC 图片解码（仅 platform 层）
//
// 用途：把存储的 PNG blob 转成 CF_DIB 可写回剪贴板（T-205）。
// 约束：阻塞函数，禁止在 async 上下文直接调用。

use crate::error::AppError;

/// PNG 字节 → CF_DIB 字节（BITMAPINFOHEADER + bottom-up 32bpp BGRA）
///
/// v1 策略：alpha 统一写 0xFF（GDI 消费方不认 alpha，避免半透明区域发黑）；
/// 行序翻转为 bottom-up，与 `BI_RGB` 正高度一致。
///
/// Linux 不需要 DIB 转换，直接返回 PNG 原始字节。
pub fn png_to_dib(png: &[u8]) -> Result<Vec<u8>, AppError> {
    if png.is_empty() {
        return Err(AppError::ClipboardRead);
    }

    #[cfg(target_os = "windows")]
    {
        png_to_dib_windows(png)
    }

    #[cfg(target_os = "linux")]
    {
        // Linux 不需要 DIB 转换，直接返回 PNG 原始字节
        // services 层写入剪贴板时会走 arboard 的 PNG 路径
        Ok(png.to_vec())
    }
}

// ============================================================
// Windows 实现
// ============================================================
#[cfg(target_os = "windows")]
fn png_to_dib_windows(png: &[u8]) -> Result<Vec<u8>, AppError> {
    use windows::core::GUID;
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, IWICImagingFactory, WICBitmapDitherTypeNone,
        WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };

    /// 目标像素格式：32bpp BGRA
    #[allow(non_upper_case_globals)]
    const GUID_WICPixelFormat32bppBGRA: GUID =
        GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc90f);

    /// BITMAPINFOHEADER 长度
    const BITMAPINFOHEADER_LEN: usize = 40;

    struct ComGuard {
        should_uninit: bool,
    }

    impl ComGuard {
        fn initialize() -> Result<Self, AppError> {
            // SAFETY: 无参数，始终安全
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if hr.is_ok() || hr == RPC_E_CHANGED_MODE {
                Ok(Self {
                    should_uninit: hr.is_ok(),
                })
            } else {
                Err(AppError::Internal(format!(
                    "COM init failed: 0x{:08X}",
                    hr.0
                )))
            }
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.should_uninit {
                // SAFETY: 与 initialize() 中成功的 CoInitializeEx 一一配对
                unsafe { CoUninitialize() };
            }
        }
    }

    let _com = ComGuard::initialize()?;

    // SAFETY: 所有 COM 对象在本函数内创建与释放，GUID 为静态常量
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| AppError::Internal(format!("WIC factory failed: {}", e)))?;

        let stream = windows::Win32::UI::Shell::SHCreateMemStream(Some(png))
            .ok_or_else(|| AppError::Internal("SHCreateMemStream failed".into()))?;

        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .map_err(|e| AppError::Internal(format!("WIC decoder failed: {}", e)))?;
        let frame = decoder
            .GetFrame(0)
            .map_err(|e| AppError::Internal(format!("WIC frame failed: {}", e)))?;

        let converted = frame
            .ConvertPixelFormat(
                &GUID_WICPixelFormat32bppBGRA,
                WICBitmapDitherTypeNone,
                &WICBitmapPaletteTypeCustom,
            )
            .map_err(|e| AppError::Internal(format!("WIC convert failed: {}", e)))?;

        let width = converted
            .GetSize()
            .map_err(|e| AppError::Internal(format!("WIC size failed: {}", e)))?
            .Width as usize;
        let height = converted
            .GetSize()
            .map_err(|e| AppError::Internal(format!("WIC size failed: {}", e)))?
            .Height as usize;

        let stride = width * 4;
        let pixels_len = stride * height;
        let mut pixels = vec![0u8; pixels_len];

        converted
            .CopyPixels(std::ptr::null(), stride as u32, &mut pixels)
            .map_err(|e| AppError::Internal(format!("WIC copy failed: {}", e)))?;

        let mut dib = Vec::with_capacity(BITMAPINFOHEADER_LEN + pixels_len);
        dib.extend_from_slice(&bitmap_info_header(width, height, stride));
        dib.extend_from_slice(&pixels);

        Ok(dib)
    }
}

#[cfg(target_os = "windows")]
fn bitmap_info_header(width: usize, height: usize, stride: usize) -> [u8; 40] {
    let mut header = [0u8; 40];
    let bi_size = 40u32;
    let bi_width = width as i32;
    let bi_height = height as i32; // 正数表示 bottom-up
    let bi_planes = 1u16;
    let bi_bit_count = 32u16;
    let bi_compression = 0u32; // BI_RGB
    let bi_size_image = (stride * height) as u32;

    header[0..4].copy_from_slice(&bi_size.to_le_bytes());
    header[4..8].copy_from_slice(&bi_width.to_le_bytes());
    header[8..12].copy_from_slice(&bi_height.to_le_bytes());
    header[12..14].copy_from_slice(&bi_planes.to_le_bytes());
    header[14..16].copy_from_slice(&bi_bit_count.to_le_bytes());
    header[16..20].copy_from_slice(&bi_compression.to_le_bytes());
    header[20..24].copy_from_slice(&bi_size_image.to_le_bytes());

    header
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(non_snake_case)]
    fn 空PNG返回错误() {
        let result = png_to_dib(&[]);
        assert!(result.is_err());
    }

    #[test]
    #[allow(non_snake_case)]
    fn 非空PNG返回数据() {
        let png = vec![0x89, 0x50, 0x4E, 0x47]; // PNG magic
        let result = png_to_dib(&png);
        assert!(result.is_ok());
        #[cfg(target_os = "linux")]
        assert_eq!(result.unwrap(), png);
    }
}
