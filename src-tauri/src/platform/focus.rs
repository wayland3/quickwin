// 焦点与显示器管理
//
// Windows: Win32 API 实现
// Linux: 降级实现（TODO: 接入 xcb + EWMH）

use crate::error::AppError;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ForegroundWindow {
    pub hwnd: isize,
    pub pid: u32,
    pub process_name: String,
    pub window_title: String,
    pub is_elevated: bool,
    pub is_minimized: bool,
    pub monitor: Option<String>,
    pub focus_child: Option<isize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MonitorInfo {
    pub work_left: i32,
    pub work_top: i32,
    pub work_width: i32,
    pub work_height: i32,
    pub dpi: u32,
    pub scale_factor: f64,
}

/// 捕获当前前台窗口信息
pub fn capture_foreground() -> Option<ForegroundWindow> {
    #[cfg(target_os = "windows")]
    {
        capture_foreground_windows()
    }

    #[cfg(target_os = "linux")]
    {
        // TODO: 接入 xcb + EWMH (_NET_ACTIVE_WINDOW) 实现
        None
    }
}

/// 检测当前进程是否提权
pub fn current_process_elevated() -> bool {
    #[cfg(target_os = "windows")]
    {
        current_process_elevated_windows()
    }

    #[cfg(target_os = "linux")]
    {
        // Linux 无 UAC 概念，始终返回 false
        false
    }
}

/// 恢复焦点到之前的窗口
pub fn restore(prev: &ForegroundWindow, timeout_ms: u32) -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        restore_windows(prev, timeout_ms)
    }

    #[cfg(target_os = "linux")]
    {
        // Linux 降级实现：暂不做焦点恢复，调用方按「已复制，请手动粘贴」处理。
        // TODO: 接入 xcb + XSetInputFocus 实现
        let _ = (prev, timeout_ms);
        Ok(())
    }
}

/// 释放卡住的修饰键
pub fn release_stuck_modifiers(prev: &ForegroundWindow) {
    #[cfg(target_os = "windows")]
    {
        release_stuck_modifiers_windows(prev)
    }

    #[cfg(target_os = "linux")]
    {
        // Linux 降级实现：暂无卡键问题。
        // TODO: 接入 xcb + XQueryKeymap 实现
        let _ = prev;
    }
}

/// 获取鼠标所在显示器信息
pub fn get_cursor_monitor() -> Result<MonitorInfo, AppError> {
    #[cfg(target_os = "windows")]
    {
        get_cursor_monitor_windows()
    }

    #[cfg(target_os = "linux")]
    {
        // TODO: 接入 xcb 或 xrandr 获取真实显示器信息
        Ok(MonitorInfo {
            work_left: 0,
            work_top: 0,
            work_width: 1920,
            work_height: 1080,
            dpi: 96,
            scale_factor: 1.0,
        })
    }
}

/// 计算浮层位置（纯函数，跨平台通用）
pub fn calculate_overlay_position(
    work_left: i32,
    work_top: i32,
    work_width: i32,
    work_height: i32,
    scale_factor: f64,
    window_width: u32,
    window_height: u32,
) -> (i32, i32, u32, u32) {
    // 工作区转逻辑像素
    let logical_work_left = work_left as f64 / scale_factor;
    let logical_work_top = work_top as f64 / scale_factor;
    let logical_work_width = work_width as f64 / scale_factor;
    let logical_work_height = work_height as f64 / scale_factor;

    // 竖屏限制宽度
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

// ============================================================
// Windows 实现
// ============================================================
#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use std::path::PathBuf;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{BOOL, FALSE, HWND, LPARAM, TRUE};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{
        AttachThreadInput, GetCurrentThreadId, OpenProcess, OpenProcessToken,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
        IsWindow, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    pub fn capture_foreground_windows() -> Option<ForegroundWindow> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_invalid() {
            return None;
        }

        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };

        let process_name = get_process_name(pid).unwrap_or_else(|| "unknown".to_string());
        let is_elevated = check_elevated(pid);
        let is_minimized = unsafe { IsIconic(hwnd) }.as_bool();
        let focus_child = get_focus_child(hwnd);

        Some(ForegroundWindow {
            hwnd: hwnd.0 as isize,
            pid,
            process_name,
            window_title: String::new(),
            is_elevated,
            is_minimized,
            monitor: None,
            focus_child,
        })
    }

    fn get_process_name(pid: u32) -> Option<String> {
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid) }.ok()?;

        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let result = unsafe {
            windows::Win32::System::Threading::QueryFullProcessImageNameW(
                process,
                windows::Win32::System::Threading::PROCESS_NAME_FORMAT(0),
                PWSTR(buf.as_mut_ptr()),
                &mut size,
            )
        };

        if result.is_err() {
            return None;
        }

        let path = String::from_utf16_lossy(&buf[..size as usize]);
        let name = PathBuf::from(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // UWP 检测：ApplicationFrameHost
        if name.eq_ignore_ascii_case("ApplicationFrameHost.exe") {
            return detect_uwp_app_name(pid);
        }

        Some(name)
    }

    fn detect_uwp_app_name(pid: u32) -> Option<String> {
        // 简化版：返回 "UWP App" 占位
        Some("UWP App".to_string())
    }

    fn check_elevated(pid: u32) -> bool {
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid) }.ok()?;
        let mut token = windows::Win32::Foundation::HANDLE::default();
        let result = unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) };
        if result.is_err() {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let result = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut ret_len,
            )
        };

        result.is_ok() && elevation.TokenIsElevated != 0
    }

    fn get_focus_child(hwnd: HWND) -> Option<isize> {
        let mut thread_id = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut thread_id)) };

        // SAFETY: 当前线程 ID 有效
        let current_thread = unsafe { GetCurrentThreadId() };
        if current_thread == thread_id {
            let focus = unsafe { GetFocus() };
            if !focus.is_invalid() && unsafe { IsWindow(focus) }.as_bool() {
                return Some(focus.0 as isize);
            }
        }

        // 跨线程：用 EnumChildWindows + GetFocus
        struct FocusState {
            thread_id: u32,
            focus: Option<HWND>,
        }

        let mut state = FocusState {
            thread_id,
            focus: None,
        };

        unsafe {
            EnumChildWindows(
                hwnd,
                Some(enum_focus_callback),
                LPARAM(&mut state as *mut _ as isize),
            )
        };

        state.focus.map(|h| h.0 as isize)
    }

    unsafe extern "system" fn enum_focus_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = &mut *(lparam.0 as *mut FocusState);
        let mut thread_id = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut thread_id));

        if thread_id == state.thread_id {
            let focus = GetFocus();
            if !focus.is_invalid() && IsWindow(focus).as_bool() {
                state.focus = Some(focus);
                return FALSE; // 停止枚举
            }
        }

        TRUE // 继续枚举
    }

    pub fn current_process_elevated_windows() -> bool {
        let mut token = windows::Win32::Foundation::HANDLE::default();
        let result = unsafe {
            OpenProcessToken(
                windows::Win32::System::Threading::GetCurrentProcess(),
                TOKEN_QUERY,
                &mut token,
            )
        };
        if result.is_err() {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let result = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut ret_len,
            )
        };

        result.is_ok() && elevation.TokenIsElevated != 0
    }

    pub fn restore_windows(prev: &ForegroundWindow, timeout_ms: u32) -> Result<(), AppError> {
        let hwnd = HWND(prev.hwnd as *mut _);
        if !unsafe { IsWindow(hwnd) }.as_bool() {
            return Ok(());
        }

        // 如果窗口最小化，先恢复
        if prev.is_minimized {
            unsafe { ShowWindow(hwnd, SW_RESTORE) };
        }

        // 附加线程输入
        let current_thread = unsafe { GetCurrentThreadId() };
        let target_thread = unsafe { GetWindowThreadProcessId(hwnd, None) };

        if current_thread != target_thread {
            unsafe { AttachThreadInput(current_thread, target_thread, TRUE) };
        }

        // 设置前台窗口
        let result = unsafe { SetForegroundWindow(hwnd) };

        // 分离线程输入
        if current_thread != target_thread {
            unsafe { AttachThreadInput(current_thread, target_thread, FALSE) };
        }

        if !result.as_bool() {
            return Err(AppError::Internal("SetForegroundWindow failed".into()));
        }

        Ok(())
    }

    pub fn release_stuck_modifiers_windows(_prev: &ForegroundWindow) {
        // TODO: 实现修饰键释放
    }

    pub fn get_cursor_monitor_windows() -> Result<MonitorInfo, AppError> {
        let mut pos = windows::Win32::Graphics::Gdi::POINT { x: 0, y: 0 };
        unsafe { GetCursorPos(&mut pos) }?;

        let monitor = unsafe { MonitorFromPoint(pos, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };

        unsafe { GetMonitorInfoW(monitor, &mut info) };

        let mut dpi_x = 96u32;
        let mut dpi_y = 96u32;
        unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }?;

        let scale_factor = dpi_x as f64 / 96.0;

        Ok(MonitorInfo {
            work_left: info.rcWork.left,
            work_top: info.rcWork.top,
            work_width: info.rcWork.right - info.rcWork.left,
            work_height: info.rcWork.bottom - info.rcWork.top,
            dpi: dpi_x,
            scale_factor,
        })
    }
}

#[cfg(target_os = "windows")]
use windows_impl::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 标准显示器居中计算() {
        let (x, y, w, h) = calculate_overlay_position(0, 0, 1920, 1080, 1.0, 720, 400);
        assert_eq!(x, 600);
        assert_eq!(y, 216);
        assert_eq!(w, 720);
        assert_eq!(h, 400);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 高DPI缩放计算() {
        let (x, y, w, h) = calculate_overlay_position(0, 0, 2880, 1620, 1.5, 720, 400);
        assert_eq!(x, 600);
        assert_eq!(y, 216);
        assert_eq!(w, 720);
        assert_eq!(h, 400);
    }
}
