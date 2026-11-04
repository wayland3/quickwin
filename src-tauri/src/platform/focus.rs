// 焦点与显示器管理
//
// Windows: Win32 API 实现
// Linux: 直接链接系统 libX11（不新增 crate），用 EWMH 属性 + XSetInputFocus

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
        linux_impl::capture_foreground_linux()
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
        linux_impl::restore_linux(prev, timeout_ms)
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
        linux_impl::release_stuck_modifiers_linux(prev)
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
        linux_impl::get_cursor_monitor_linux()
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

// ============================================================
// Linux 实现（直接链接系统 libX11）
// ============================================================
//
// 为什么不引入 `xcb` / `x11rb`：按 `AGENTS.md` §13，新增 crate 需要 ADR + 用户确认。
// 焦点捕获/恢复只需要极少数 Xlib 调用，直接 `extern "C"` 链接系统 libX11 即可，
// 桌面环境必然安装该库，不引入依赖、不增加编译量。
#[cfg(target_os = "linux")]
mod linux_impl {
    use super::ForegroundWindow;
    use super::MonitorInfo;
    use crate::error::AppError;
    use std::ffi::c_void;
    use std::os::raw::{c_char, c_int, c_long, c_uchar, c_ulong};
    use std::sync::Mutex;

    /// Xlib `Display*`
    type Display = *mut c_void;
    /// XID / Window / Atom / Time 都是 `unsigned long`
    type XId = c_ulong;

    /// `RevertToPointerRoot`
    const REVERT_TO_POINTER_ROOT: c_int = 1;
    /// `CurrentTime`
    const CURRENT_TIME: c_ulong = 0;
    /// `AnyPropertyType`
    const ANY_PROPERTY_TYPE: XId = 0;
    /// `PointerRoot`（焦点在根窗口时 `XGetInputFocus` 返回它）
    const POINTER_ROOT: XId = 1;

    #[link(name = "X11")]
    extern "C" {
        fn XOpenDisplay(name: *const c_char) -> Display;
        fn XCloseDisplay(display: Display) -> c_int;
        fn XDefaultScreen(display: Display) -> c_int;
        fn XDefaultRootWindow(display: Display) -> XId;
        fn XDisplayWidth(display: Display, screen: c_int) -> c_int;
        fn XDisplayHeight(display: Display, screen: c_int) -> c_int;
        fn XDisplayWidthMM(display: Display, screen: c_int) -> c_int;
        fn XGetInputFocus(
            display: Display,
            focus_return: *mut XId,
            revert_to_return: *mut c_int,
        ) -> c_int;
        fn XSetInputFocus(display: Display, focus: XId, revert_to: c_int, time: c_ulong) -> c_int;
        fn XRaiseWindow(display: Display, window: XId) -> c_int;
        fn XInternAtom(display: Display, atom_name: *const c_char, only_if_exists: c_int) -> XId;
        fn XGetWindowProperty(
            display: Display,
            window: XId,
            property: XId,
            long_offset: c_long,
            long_length: c_long,
            delete: c_int,
            req_type: XId,
            actual_type_return: *mut XId,
            actual_format_return: *mut c_int,
            nitems_return: *mut c_ulong,
            bytes_after_return: *mut c_ulong,
            prop_return: *mut *mut c_uchar,
        ) -> c_int;
        fn XFree(data: *mut c_void) -> c_int;
        fn XFlush(display: Display) -> c_int;
    }

    /// X 连接守卫：离开作用域自动 `XCloseDisplay`
    struct XDisplay {
        ptr: Display,
    }

    impl XDisplay {
        fn open() -> Result<Self, AppError> {
            // SAFETY: 传 null 表示使用 $DISPLAY；返回值可能是 null（无 X 会话）
            let ptr = unsafe { XOpenDisplay(std::ptr::null()) };
            if ptr.is_null() {
                return Err(AppError::Internal("无法连接 X11 显示服务".into()));
            }
            Ok(Self { ptr })
        }
    }

    impl Drop for XDisplay {
        fn drop(&mut self) {
            // SAFETY: ptr 由 XOpenDisplay 创建，只在此处关闭一次
            unsafe { XCloseDisplay(self.ptr) };
        }
    }

    /// 读取 32 位属性数组（如 `_NET_ACTIVE_WINDOW`、`_NET_WM_STATE`、`_NET_WORKAREA`）
    fn read_cardinals(display: Display, window: XId, name: &str) -> Option<Vec<XId>> {
        let atom_name = std::ffi::CString::new(name).ok()?;
        // SAFETY: atom_name 在调用期间有效；only_if_exists=0 表示必要时创建
        let atom = unsafe { XInternAtom(display, atom_name.as_ptr(), 0) };
        if atom == 0 {
            return None;
        }

        let mut actual_type: XId = 0;
        let mut actual_format: c_int = 0;
        let mut nitems: c_ulong = 0;
        let mut bytes_after: c_ulong = 0;
        let mut prop: *mut c_uchar = std::ptr::null_mut();

        // SAFETY: 所有输出指针指向本函数栈上的有效变量；delete=0 不改变属性
        let status = unsafe {
            XGetWindowProperty(
                display,
                window,
                atom,
                0,
                256,
                0,
                ANY_PROPERTY_TYPE,
                &mut actual_type,
                &mut actual_format,
                &mut nitems,
                &mut bytes_after,
                &mut prop,
            )
        };
        if status != 0 || prop.is_null() {
            if !prop.is_null() {
                // SAFETY: prop 由 Xlib 分配，必须用 XFree 释放
                unsafe { XFree(prop as *mut c_void) };
            }
            return None;
        }

        // 格式 32 的数据在 64 位 Xlib 里按 long 存放
        let mut values = Vec::new();
        if actual_format == 32 && nitems > 0 {
            // SAFETY: prop 指向 nitems 个 c_ulong（format=32）
            let slice = unsafe { std::slice::from_raw_parts(prop as *const XId, nitems as usize) };
            values.extend_from_slice(slice);
        }
        // SAFETY: 数据已复制，释放 Xlib 分配的缓冲区
        unsafe { XFree(prop as *mut c_void) };

        Some(values)
    }

    /// 读取进程名（`/proc/<pid>/comm`），不读窗口标题（隐私，`AGENTS.md` §10）
    fn process_name(pid: u32) -> String {
        if pid == 0 {
            return String::new();
        }
        std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .map(|text| text.trim().to_string())
            .unwrap_or_default()
    }

    /// 捕获当前前台窗口（EWMH `_NET_ACTIVE_WINDOW` 优先，回退 `XGetInputFocus`）
    pub fn capture_foreground_linux() -> Option<ForegroundWindow> {
        let display = XDisplay::open().ok()?;
        // SAFETY: display 有效
        let root = unsafe { XDefaultRootWindow(display.ptr) };

        // 1) EWMH：窗口管理器维护的「活动窗口」，最可靠
        let mut window = read_cardinals(display.ptr, root, "_NET_ACTIVE_WINDOW")
            .and_then(|values| values.first().copied())
            .unwrap_or(0);

        // 2) 回退：X 焦点窗口（根窗口/None 视为无目标）
        if window == 0 || window == POINTER_ROOT {
            let mut focus: XId = 0;
            let mut revert: c_int = 0;
            // SAFETY: display 有效，输出指针指向栈变量
            unsafe { XGetInputFocus(display.ptr, &mut focus, &mut revert) };
            window = focus;
        }
        if window == 0 {
            return None;
        }

        let pid = read_cardinals(display.ptr, window, "_NET_WM_PID")
            .and_then(|values| values.first().copied())
            .map(|value| value as u32)
            .unwrap_or(0);

        // 最小化：`_NET_WM_STATE` 含 `_NET_WM_STATE_HIDDEN`
        let is_minimized = read_cardinals(display.ptr, window, "_NET_WM_STATE")
            .map(|states| {
                states
                    .iter()
                    .any(|atom| atom_name_is(display.ptr, *atom, "_NET_WM_STATE_HIDDEN"))
            })
            .unwrap_or(false);

        Some(ForegroundWindow {
            hwnd: window as isize,
            pid,
            process_name: process_name(pid),
            // 不采集窗口标题（隐私约束）
            window_title: String::new(),
            is_elevated: false,
            is_minimized,
            monitor: None,
            focus_child: None,
        })
    }

    /// 判断某个 atom id 是否等于给定名字
    fn atom_name_is(display: Display, atom: XId, expected: &str) -> bool {
        let Ok(name) = std::ffi::CString::new(expected) else {
            return false;
        };
        // SAFETY: name 在调用期间有效；only_if_exists=1 不创建
        let expected_atom = unsafe { XInternAtom(display, name.as_ptr(), 1) };
        expected_atom != 0 && expected_atom == atom
    }

    /// 恢复焦点到之前捕获的窗口
    ///
    /// 先 `XRaiseWindow` 再把输入焦点设回目标窗口，然后轮询最多 `timeout_ms`
    /// 确认焦点确实切换（X 是异步的，需要给窗口管理器一点时间）。
    /// 局部静态锁：Xlib 连接不可跨线程共享，串行化所有调用避免并发关闭。
    pub fn restore_linux(prev: &ForegroundWindow, timeout_ms: u32) -> Result<(), AppError> {
        static X_LOCK: Mutex<()> = Mutex::new(());
        let _guard = X_LOCK
            .lock()
            .map_err(|_| AppError::Internal("X11 焦点锁被污染".into()))?;

        let window = prev.hwnd as XId;
        if window == 0 {
            return Ok(());
        }

        let display = XDisplay::open()?;

        // SAFETY: display 有效，window 来自之前的 XID
        unsafe {
            XRaiseWindow(display.ptr, window);
            XSetInputFocus(display.ptr, window, REVERT_TO_POINTER_ROOT, CURRENT_TIME);
            XFlush(display.ptr);
        }

        // 轮询确认焦点已切换（最多 timeout_ms，步长 10ms）
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(u64::from(timeout_ms.min(150)));
        loop {
            let mut focus: XId = 0;
            let mut revert: c_int = 0;
            // SAFETY: display 有效，输出指针指向栈变量
            unsafe { XGetInputFocus(display.ptr, &mut focus, &mut revert) };
            if focus == window {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                // 超时：窗口管理器可能拒绝切换（例如目标已关闭）
                return Err(AppError::Internal("恢复焦点超时".into()));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// 释放卡住的修饰键（Linux 暂无卡键问题，保留空实现）
    pub fn release_stuck_modifiers_linux(_prev: &ForegroundWindow) {}

    /// 获取鼠标所在工作区与 DPI
    ///
    /// 工作区用 EWMH `_NET_WORKAREA`（扣除任务栏/面板）；DPI 由屏幕物理尺寸推算。
    pub fn get_cursor_monitor_linux() -> Result<MonitorInfo, AppError> {
        let display = XDisplay::open()?;
        // SAFETY: display 有效
        let screen = unsafe { XDefaultScreen(display.ptr) };
        // SAFETY: display 有效，screen 来自同一连接
        let root = unsafe { XDefaultRootWindow(display.ptr) };
        let width_px = unsafe { XDisplayWidth(display.ptr, screen) } as i32;
        let height_px = unsafe { XDisplayHeight(display.ptr, screen) } as i32;
        let width_mm = unsafe { XDisplayWidthMM(display.ptr, screen) }.max(0) as f64;

        // 工作区：x, y, w, h（每个桌面 4 个值，取第一个）
        let work = read_cardinals(display.ptr, root, "_NET_WORKAREA").unwrap_or_default();
        let (work_left, work_top, work_width, work_height) = if work.len() >= 4 {
            (
                work[0] as i32,
                work[1] as i32,
                work[2] as i32,
                work[3] as i32,
            )
        } else {
            (0, 0, width_px, height_px)
        };

        // DPI：1 英寸 = 25.4mm；物理宽度为 0 时回退 96
        let dpi = if width_mm > 1.0 {
            (width_px as f64 / (width_mm / 25.4)).round() as u32
        } else {
            96
        };
        let dpi = dpi.max(72);
        let scale_factor = (dpi as f64 / 96.0).max(1.0);

        Ok(MonitorInfo {
            work_left,
            work_top,
            work_width,
            work_height,
            dpi,
            scale_factor,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// 真机手测：`cargo test --lib 真机捕获并恢复焦点 -- --ignored --nocapture`
        #[test]
        #[allow(non_snake_case)]
        #[ignore = "真机手测：需要可用的 X11 会话"]
        fn 真机捕获并恢复焦点() {
            let captured = capture_foreground_linux();
            println!("captured={captured:?}");
            let monitor = get_cursor_monitor_linux();
            println!("monitor={monitor:?}");
            if let Some(prev) = captured {
                let result = restore_linux(&prev, 150);
                println!("restore={result:?}");
            }
        }
    }
}

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
