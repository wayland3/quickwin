// 焦点与显示器相关 Win32 调用

use crate::error::AppError;
use serde::Serialize;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

#[derive(Debug, Clone, Serialize)]
pub struct MonitorInfo {
    pub work_left: i32,
    pub work_top: i32,
    pub work_width: i32,
    pub work_height: i32,
    pub dpi: u32,
    pub scale_factor: f64,
}

/// 获取鼠标所在显示器的信息（工作区 + DPI）
pub fn get_cursor_monitor() -> Result<MonitorInfo, AppError> {
    // SAFETY: GetCursorPos 是安全的 Win32 调用
    let point = unsafe {
        let mut pt = std::mem::zeroed();
        if GetCursorPos(&mut pt).is_err() {
            return Err(AppError::Internal("GetCursorPos failed".into()));
        }
        pt
    };

    // SAFETY: MonitorFromPoint 是安全的 Win32 调用
    let hmonitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    if hmonitor.is_invalid() {
        return Err(AppError::Internal("MonitorFromPoint failed".into()));
    }

    get_monitor_info(hmonitor)
}

/// 获取指定显示器的信息
fn get_monitor_info(hmonitor: HMONITOR) -> Result<MonitorInfo, AppError> {
    // SAFETY: GetMonitorInfoW 是安全的 Win32 调用
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let success = unsafe { GetMonitorInfoW(hmonitor, &mut mi) };
    if success.0 == 0 {
        return Err(AppError::Internal("GetMonitorInfoW failed".into()));
    }

    // SAFETY: GetDpiForMonitor 是安全的 Win32 调用
    let (dpi_x, _dpi_y) = unsafe {
        let mut x = 0;
        let mut y = 0;
        if GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &mut x, &mut y).is_err() {
            return Err(AppError::Internal("GetDpiForMonitor failed".into()));
        }
        (x, y)
    };

    let scale_factor = dpi_x as f64 / 96.0;

    Ok(MonitorInfo {
        work_left: mi.rcWork.left,
        work_top: mi.rcWork.top,
        work_width: mi.rcWork.right - mi.rcWork.left,
        work_height: mi.rcWork.bottom - mi.rcWork.top,
        dpi: dpi_x,
        scale_factor,
    })
}

/// 计算浮层位置（纯函数，可单测）
///
/// 输入：显示器工作区（物理像素）、目标 DPI、窗口逻辑宽高
/// 输出：逻辑坐标 (x, y, width, height)
pub fn calculate_overlay_position(
    work_left: i32,
    work_top: i32,
    work_width: i32,
    work_height: i32,
    scale_factor: f64,
    window_width: u32,
    window_height: u32,
) -> (i32, i32, u32, u32) {
    // 工作区转为逻辑像素
    let logical_work_left = work_left as f64 / scale_factor;
    let logical_work_top = work_top as f64 / scale_factor;
    let logical_work_width = work_width as f64 / scale_factor;
    let logical_work_height = work_height as f64 / scale_factor;

    // 竖屏时限制宽度
    let effective_width = if logical_work_width < 800.0 {
        (logical_work_width - 32.0).min(window_width as f64) as u32
    } else {
        window_width
    };

    // 水平居中，垂直距顶 20%
    let x = (logical_work_left + (logical_work_width - effective_width as f64) / 2.0) as i32;
    let y = (logical_work_top + logical_work_height * 0.2) as i32;

    (x, y, effective_width, window_height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(non_snake_case)]
    fn 标准显示器居中计算() {
        // 1920x1080, 100% DPI
        let (x, y, w, h) = calculate_overlay_position(0, 0, 1920, 1080, 1.0, 720, 400);
        assert_eq!(x, 600); // (1920 - 720) / 2
        assert_eq!(y, 216); // 1080 * 0.2
        assert_eq!(w, 720);
        assert_eq!(h, 400);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 高DPI缩放计算() {
        // 1920x1080, 150% DPI (144 DPI)
        let (x, y, w, h) = calculate_overlay_position(0, 0, 2880, 1620, 1.5, 720, 400);
        // 逻辑工作区 = 1920x1080
        assert_eq!(x, 600);
        assert_eq!(y, 216);
        assert_eq!(w, 720);
        assert_eq!(h, 400);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 竖屏宽度限制() {
        // 1080x1920 竖屏, 100% DPI
        let (_x, _y, w, h) = calculate_overlay_position(0, 0, 1080, 1920, 1.0, 720, 400);
        // 逻辑工作区宽度 1080 < 800 不成立，但 1080 - 32 = 1048 > 720
        // 实际测试：1080 > 800，所以不限制
        assert_eq!(w, 720);
        assert_eq!(h, 400);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 窄屏宽度限制() {
        // 600x1000 窄屏, 100% DPI
        let (_x, _y, w, h) = calculate_overlay_position(0, 0, 600, 1000, 1.0, 720, 400);
        // 逻辑工作区宽度 600 < 800，限制为 600 - 32 = 568
        assert_eq!(w, 568);
        assert_eq!(h, 400);
    }
}
