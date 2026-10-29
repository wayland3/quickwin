// WIC 图片解码（仅 platform 层）
//
// 用途：把存储的 PNG blob 转成 CF_DIB 可写回剪贴板（T-205）。
// 约束：阻塞函数，禁止在 async 上下文直接调用。

use crate::error::AppError;
use windows::core::GUID;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, IWICImagingFactory, WICBitmapDitherTypeNone,
    WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

/// 目标像素格式：32bpp BGRA
#[allow(non_upper_case_globals)]
const GUID_WICPixelFormat32bppBGRA: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc90f);

/// BITMAPINFOHEADER 长度
const BITMAPINFOHEADER_LEN: usize = 40;

/// PNG 字节 → CF_DIB 字节（BITMAPINFOHEADER + bottom-up 32bpp BGRA）
///
/// v1 策略：alpha 统一写 0xFF（GDI 消费方不认 alpha，避免半透明区域发黑）；
/// 行序翻转为 bottom-up，与 `BI_RGB` 正高度一致。
pub fn png_to_dib(png: &[u8]) -> Result<Vec<u8>, AppError> {
    if png.is_empty() {
        return Err(AppError::ClipboardRead);
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

        let converter = factory
            .CreateFormatConverter()
            .map_err(|e| AppError::Internal(format!("WIC converter failed: {}", e)))?;
        converter
            .Initialize(
                &frame,
                &GUID_WICPixelFormat32bppBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(|e| AppError::Internal(format!("WIC convert failed: {}", e)))?;

        let mut width = 0u32;
        let mut height = 0u32;
        converter
            .GetSize(&mut width, &mut height)
            .map_err(|e| AppError::Internal(format!("WIC size failed: {}", e)))?;
        if width == 0 || height == 0 {
            return Err(AppError::ClipboardRead);
        }

        let stride = width
            .checked_mul(4)
            .ok_or_else(|| AppError::Internal("image too wide".into()))?;
        let total = (stride as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| AppError::Internal("image too large".into()))?;
        let mut bgra = vec![0u8; total];
        converter
            .CopyPixels(std::ptr::null(), stride, &mut bgra)
            .map_err(|e| AppError::Internal(format!("WIC copy failed: {}", e)))?;

        Ok(build_dib_from_bgra(width, height, &bgra))
    }
}

/// 32bpp BGRA（top-down）→ CF_DIB（bottom-up，alpha 置 0xFF）
fn build_dib_from_bgra(width: u32, height: u32, bgra: &[u8]) -> Vec<u8> {
    let stride = (width as usize) * 4;
    let mut out = Vec::with_capacity(BITMAPINFOHEADER_LEN + stride * height as usize);

    out.extend_from_slice(&(BITMAPINFOHEADER_LEN as u32).to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    out.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&((stride * height as usize) as u32).to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes()); // x ppm
    out.extend_from_slice(&0i32.to_le_bytes()); // y ppm
    out.extend_from_slice(&0u32.to_le_bytes()); // clrUsed
    out.extend_from_slice(&0u32.to_le_bytes()); // clrImportant

    for row in (0..height as usize).rev() {
        let start = row * stride;
        for pixel in bgra[start..start + stride].as_chunks::<4>().0 {
            out.extend_from_slice(&pixel[0..3]);
            out.push(0xFF);
        }
    }

    out
}

/// COM 初始化守卫：仅当本线程由本函数初始化成功时才 CoUninitialize
struct ComGuard(bool);

impl ComGuard {
    fn initialize() -> Result<Self, AppError> {
        // SAFETY: CoInitializeEx 无副作用地登记线程 COM 上下文
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr.is_ok() {
            return Ok(Self(true));
        }
        if hr == RPC_E_CHANGED_MODE {
            // 线程已用其他模型初始化，直接复用
            return Ok(Self(false));
        }
        Err(AppError::Internal(format!(
            "CoInitializeEx failed: 0x{:08X}",
            hr.0
        )))
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: 与 initialize 成对
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(non_snake_case)]
    fn DIB头字段与行序正确() {
        // 2x2：第一行（top）全红，第二行全蓝
        let mut bgra = Vec::new();
        for _ in 0..2 {
            bgra.extend_from_slice(&[0, 0, 255, 0x80]); // B,G,R,A
        }
        for _ in 0..2 {
            bgra.extend_from_slice(&[255, 0, 0, 0x40]);
        }

        let dib = build_dib_from_bgra(2, 2, &bgra);
        assert_eq!(dib.len(), BITMAPINFOHEADER_LEN + 2 * 2 * 4);
        assert_eq!(
            u32::from_le_bytes(dib[0..4].try_into().unwrap_or_default()),
            40
        );
        assert_eq!(
            i32::from_le_bytes(dib[4..8].try_into().unwrap_or_default()),
            2
        );
        assert_eq!(
            i32::from_le_bytes(dib[8..12].try_into().unwrap_or_default()),
            2
        );

        // 第一行像素是 top 的最后一行（蓝），alpha 强置 0xFF
        let first_pixel = &dib[BITMAPINFOHEADER_LEN..BITMAPINFOHEADER_LEN + 4];
        assert_eq!(first_pixel, &[255, 0, 0, 0xFF]);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 一像素PNG可转DIB() {
        // 1x1 红色 PNG
        const PNG: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08,
            0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0xFF, 0x9F, 0x9B,
            0x99, 0x99, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];

        let dib = match png_to_dib(PNG) {
            Ok(dib) => dib,
            Err(e) => panic!("png_to_dib failed: {}", e),
        };
        assert_eq!(
            i32::from_le_bytes(dib[4..8].try_into().unwrap_or_default()),
            1
        );
        assert_eq!(
            i32::from_le_bytes(dib[8..12].try_into().unwrap_or_default()),
            1
        );
        assert_eq!(dib[BITMAPINFOHEADER_LEN + 3], 0xFF);
    }
}
