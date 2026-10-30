// 焦点与显示器相关 Win32 调用

use crate::error::AppError;
use serde::Serialize;
use std::path::PathBuf;
use windows::core::PWSTR;
use windows::Win32::Foundation::{BOOL, FALSE, HWND, LPARAM, TRUE};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
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

/// 捕获当前前台窗口信息（失败降级为 None）
pub fn capture_foreground() -> Option<ForegroundWindow> {
    // SAFETY: GetForegroundWindow 无参数
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }

    let mut pid = 0u32;
    // SAFETY: hwnd 有效
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };

    // 获取进程名
    let process_name = get_process_name(pid).unwrap_or_else(|| "unknown".to_string());

    // 检测提权（TokenElevation 为主）
    let is_elevated = check_elevated(pid);

    // 检测最小化
    // SAFETY: hwnd 有效
    let is_minimized = unsafe { IsIconic(hwnd) }.as_bool();

    // 获取焦点子窗口
    let focus_child = get_focus_child(hwnd);

    Some(ForegroundWindow {
        hwnd: hwnd.0 as isize,
        pid,
        process_name,
        window_title: String::new(), // 不记录窗口标题到日志
        is_elevated,
        is_minimized,
        monitor: None, // TODO: 获取显示器标识
        focus_child,
    })
}

/// 获取进程名
fn get_process_name(pid: u32) -> Option<String> {
    // SAFETY: OpenProcess 是安全的 Win32 调用
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid) }.ok()?;

    let mut buf = [0u16; 1024];
    let mut size = buf.len() as u32;
    // SAFETY: process 有效，buf 为栈上可变引用
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
    if name.to_lowercase() == "applicationframehost.exe" {
        // TODO: EnumChildWindows 找真实子窗口
        // 当前返回宿主名，不影响功能
    }

    Some(name)
}

/// 检测当前进程是否提权（用于 UIPI 判断：同权限或更高权限才允许注入）
pub fn current_process_elevated() -> bool {
    // SAFETY: GetCurrentProcessId 无参数
    let pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
    check_elevated(pid)
}

/// 检测进程是否提权（TokenElevation 为主）
fn check_elevated(pid: u32) -> bool {
    // SAFETY: OpenProcess 是安全的 Win32 调用
    let process = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid) } {
        Ok(p) => p,
        Err(_) => {
            // ACCESS_DENIED 视为提权
            return true;
        }
    };

    let mut token = windows::Win32::Foundation::HANDLE::default();
    // SAFETY: process 有效
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.is_err() {
        return false;
    }

    let mut elevation = TOKEN_ELEVATION::default();
    let mut ret_len = 0u32;
    // SAFETY: token 有效，elevation 为栈上可变引用
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        )
    };

    if result.is_err() {
        return false;
    }

    elevation.TokenIsElevated != 0
}

/// 获取焦点子窗口
fn get_focus_child(hwnd: HWND) -> Option<isize> {
    struct ChildFocus {
        focus: Option<isize>,
    }

    extern "system" fn enum_child_proc(child: HWND, param: LPARAM) -> BOOL {
        // SAFETY: param 为有效指针
        let data = unsafe { &mut *(param.0 as *mut ChildFocus) };
        // SAFETY: child 有效
        if unsafe { GetFocus() } == child {
            data.focus = Some(child.0 as isize);
            return FALSE; // 停止枚举
        }
        TRUE
    }

    let mut data = ChildFocus { focus: None };
    // SAFETY: hwnd 有效，回调函数有效
    let _ = unsafe {
        EnumChildWindows(
            hwnd,
            Some(enum_child_proc),
            LPARAM(&mut data as *mut _ as isize),
        )
    };

    data.focus
}

/// RAII 守卫：AttachThreadInput 成对调用
pub struct ThreadInputGuard {
    cur_tid: u32,
    target_tid: u32,
}

impl ThreadInputGuard {
    /// 附加当前线程到目标窗口线程
    pub fn attach(hwnd: HWND) -> Result<Self, AppError> {
        // SAFETY: GetCurrentThreadId 无参数
        let cur_tid = unsafe { GetCurrentThreadId() };

        let mut target_tid = 0u32;
        // SAFETY: hwnd 有效
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut target_tid)) };

        // SAFETY: 成对调用
        let result = unsafe { AttachThreadInput(cur_tid, target_tid, TRUE) };
        if result == FALSE {
            Err(AppError::ElevatedTarget)
        } else {
            Ok(Self {
                cur_tid,
                target_tid,
            })
        }
    }
}

impl Drop for ThreadInputGuard {
    fn drop(&mut self) {
        // SAFETY: 成对调用
        let _ = unsafe { AttachThreadInput(self.cur_tid, self.target_tid, FALSE) };
    }
}

/// 恢复焦点后清理目标线程里「卡住」的修饰键
///
/// 原因：按 `Alt+V` 呼出面板时，Alt 的 keydown 发给了目标窗口线程，
/// keyup 却发给了浮层线程；切回焦点后目标线程仍认为 Alt 处于按下状态，
/// 会进入菜单模式导致后续点击被吞掉。这里附加到目标线程读取其键盘状态，
/// 只对真正处于按下状态的修饰键补发 KEYUP。
pub fn release_stuck_modifiers(prev: &ForegroundWindow) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };

    let hwnd = HWND(prev.hwnd as *mut _);
    let Ok(_guard) = ThreadInputGuard::attach(hwnd) else {
        return;
    };

    let mut state = [0u8; 256];
    // SAFETY: 已附加到目标线程，GetKeyboardState 读取共享的线程键盘状态
    if unsafe { GetKeyboardState(&mut state) }.is_err() {
        return;
    }

    let modifiers = [VK_MENU, VK_CONTROL, VK_SHIFT, VK_LWIN, VK_RWIN];
    for vk in modifiers {
        if state[vk.0 as usize] & 0x80 == 0 {
            continue;
        }
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk.0),
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        // SAFETY: input 为有效 INPUT 结构
        let _ = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        log::info!("focus: released stuck modifier vk={}", vk.0);
    }
}
pub fn restore(prev: &ForegroundWindow, timeout_ms: u32) -> Result<(), AppError> {
    let hwnd = HWND(prev.hwnd as *mut _);

    // IsWindow 校验
    // SAFETY: hwnd 可能无效
    let is_valid = unsafe { IsWindow(hwnd) };
    if is_valid == FALSE {
        return Err(AppError::Internal("window no longer exists".into()));
    }

    // 重试 3 次：先直接抢焦点（隐藏自己的窗口后本进程仍持有前台权限，
    // 通常一次即可）；失败再 AttachThreadInput 后重试
    for _ in 0..3 {
        // 最小化时先恢复
        if prev.is_minimized {
            // SAFETY: hwnd 已通过 IsWindow 校验
            let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
        }

        // SAFETY: hwnd 已通过 IsWindow 校验
        let direct = unsafe { SetForegroundWindow(hwnd) };

        if direct == FALSE {
            // SAFETY: 成对调用由 Guard 管理；附加失败不阻断后续直接重试
            if let Ok(_guard) = ThreadInputGuard::attach(hwnd) {
                if prev.is_minimized {
                    // SAFETY: hwnd 有效
                    let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
                }
                // SAFETY: hwnd 有效
                let _ = unsafe { SetForegroundWindow(hwnd) };
            }
        }

        // 焦点子窗口：跨进程可能失败，失败不影响顶级窗口激活
        if let Some(child) = prev.focus_child {
            let child_hwnd = HWND(child as *mut _);
            // SAFETY: child_hwnd 可能无效
            if unsafe { IsWindow(child_hwnd) } == TRUE {
                let _ = unsafe { SetFocus(child_hwnd) };
            }
        }

        // 校验
        // SAFETY: GetForegroundWindow 无参数
        let current = unsafe { GetForegroundWindow() };
        if current == hwnd {
            return Ok(());
        }

        std::thread::sleep(std::time::Duration::from_millis((timeout_ms / 3) as u64));
    }

    Err(AppError::Internal("focus restore timeout".into()))
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
