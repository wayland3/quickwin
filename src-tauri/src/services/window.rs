// 浮层窗口管理（状态机）

use crate::error::AppError;
use crate::platform::focus::{
    calculate_overlay_position, capture_foreground, get_cursor_monitor, release_stuck_modifiers,
    restore, ForegroundWindow,
};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, WebviewWindow, Window};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayState {
    Hidden,
    Showing, // 动画中
    Shown,
    Hiding, // 动画中
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMode {
    Launcher,
    Clipboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HideReason {
    UserCommand, // overlay_hide 命令，带恢复
    Blur,        // 失焦隐藏，不带恢复
    ModeSwitch,  // 模式切换，不带恢复
    Launch,      // 打开应用后隐藏，不恢复焦点（让新应用留在前台）
}

impl OverlayMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            OverlayMode::Launcher => "launcher",
            OverlayMode::Clipboard => "clipboard",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "launcher" => Some(OverlayMode::Launcher),
            "clipboard" => Some(OverlayMode::Clipboard),
            _ => None,
        }
    }
}

pub struct OverlayManager {
    state: Arc<Mutex<OverlayState>>,
    current_mode: Arc<Mutex<Option<OverlayMode>>>,
    hide_on_blur: Arc<Mutex<bool>>,
    prefers_reduced_motion: Arc<Mutex<bool>>,
    prev_window: Arc<Mutex<Option<ForegroundWindow>>>,
    /// 粘贴流程互斥：并发激活直接拒绝第二次
    paste_lock: Arc<Mutex<()>>,
}

/// 粘贴前恢复焦点的结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreOutcome {
    Restored,
    /// 触发浮层时没有捕获到前台窗口
    NoTarget,
    /// 目标窗口已关闭 / UIPI / 校验失败
    Failed,
}

impl OverlayManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(OverlayState::Hidden)),
            current_mode: Arc::new(Mutex::new(None)),
            hide_on_blur: Arc::new(Mutex::new(true)),
            prefers_reduced_motion: Arc::new(Mutex::new(false)),
            prev_window: Arc::new(Mutex::new(None)),
            paste_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn show(&self, window: &Window, mode: OverlayMode) -> Result<(), AppError> {
        log::info!("show: called with mode {:?}", mode);
        let mut state = self
            .state
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))?;

        log::info!("show: current state is {:?}", *state);

        // 状态机转换
        match *state {
            OverlayState::Shown => {
                // 已显示，切换语义：如果同 mode 则隐藏
                let current = self
                    .current_mode
                    .lock()
                    .map_err(|_| AppError::Internal("lock poisoned".into()))?;
                if *current == Some(mode) {
                    log::info!("show: same mode, will hide");
                    drop(state);
                    drop(current);
                    return self.hide(window, HideReason::ModeSwitch);
                }
                log::info!("show: different mode, will switch");
                // 不同 mode，直接切换（不 capture，浮层已是前台）
            }
            OverlayState::Hiding => {
                log::info!("show: state is Hiding, will cancel and show");
                // 取消隐藏，立即显示（不 capture）
            }
            OverlayState::Showing => {
                log::info!("show: already Showing, ignoring");
                // 已在显示中，忽略
                return Ok(());
            }
            OverlayState::Hidden => {
                log::info!("show: state is Hidden, will show normally");
                // 正常显示：capture 前台窗口
                let prev = capture_foreground();
                log::info!(
                    "overlay show: captured_prev={}, elevated={}",
                    prev.is_some(),
                    prev.as_ref()
                        .map(|window| window.is_elevated)
                        .unwrap_or(false)
                );
                *self
                    .prev_window
                    .lock()
                    .map_err(|_| AppError::Internal("lock poisoned".into()))? = prev;
            }
        }

        // 计算位置
        let monitor = get_cursor_monitor()?;
        let (x, y, width, height) = calculate_overlay_position(
            monitor.work_left,
            monitor.work_top,
            monitor.work_width,
            monitor.work_height,
            monitor.scale_factor,
            720,
            400,
        );

        // 设置窗口位置和大小（逻辑像素）
        window.set_size(tauri::LogicalSize::new(width, height))?;
        window.set_position(tauri::LogicalPosition::new(x, y))?;

        // 显示窗口
        window.show()?;

        // Linux 上需要延迟设置焦点，避免窗口管理器立即切换焦点
        #[cfg(target_os = "linux")]
        {
            let window_clone = window.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let _ = window_clone.set_focus();
            });
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = window.set_focus();
        }

        // 更新状态
        let reduced_motion = self.prefers_reduced_motion_reduced();
        *state = if reduced_motion {
            OverlayState::Shown
        } else {
            OverlayState::Showing
        };
        *self
            .current_mode
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))? = Some(mode);

        // 发送事件
        let _ = window.emit("overlay://shown", mode.as_str());

        // 如果有动画，延迟更新状态
        if !reduced_motion {
            let state_clone = Arc::clone(&self.state);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(120));
                if let Ok(mut s) = state_clone.lock() {
                    if *s == OverlayState::Showing {
                        *s = OverlayState::Shown;
                    }
                }
            });
        }

        Ok(())
    }

    /// 显示 WebviewWindow（用于托盘图标点击等场景）
    pub fn show_webview(&self, window: &WebviewWindow, mode: OverlayMode) -> Result<(), AppError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))?;

        // 状态机转换
        match *state {
            OverlayState::Shown => {
                // 已显示，切换语义：如果同 mode 则隐藏
                let current = self
                    .current_mode
                    .lock()
                    .map_err(|_| AppError::Internal("lock poisoned".into()))?;
                if *current == Some(mode) {
                    drop(state);
                    drop(current);
                    // 隐藏 WebviewWindow
                    let _ = window.emit("overlay://hidden", ());
                    window.hide()?;
                    *self
                        .state
                        .lock()
                        .map_err(|_| AppError::Internal("lock poisoned".into()))? =
                        OverlayState::Hidden;
                    return Ok(());
                }
                // 不同 mode，直接切换
            }
            OverlayState::Hiding => {
                // 取消隐藏，立即显示
            }
            OverlayState::Showing => {
                // 已在显示中，忽略
                return Ok(());
            }
            OverlayState::Hidden => {
                // 正常显示：capture 前台窗口
                let prev = capture_foreground();
                log::info!(
                    "overlay show (webview): captured_prev={}, elevated={}",
                    prev.is_some(),
                    prev.as_ref()
                        .map(|window| window.is_elevated)
                        .unwrap_or(false)
                );
                *self
                    .prev_window
                    .lock()
                    .map_err(|_| AppError::Internal("lock poisoned".into()))? = prev;
            }
        }

        // 计算位置
        let monitor = get_cursor_monitor()?;
        let (x, y, width, height) = calculate_overlay_position(
            monitor.work_left,
            monitor.work_top,
            monitor.work_width,
            monitor.work_height,
            monitor.scale_factor,
            720,
            400,
        );

        // 设置窗口位置和大小（逻辑像素）
        window.set_size(tauri::LogicalSize::new(width, height))?;
        window.set_position(tauri::LogicalPosition::new(x, y))?;

        // 显示窗口
        window.show()?;
        let _ = window.set_focus();

        // 更新状态
        let reduced_motion = self.prefers_reduced_motion_reduced();
        *state = if reduced_motion {
            OverlayState::Shown
        } else {
            OverlayState::Showing
        };
        *self
            .current_mode
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))? = Some(mode);

        // 发送事件
        let _ = window.emit("overlay://shown", mode.as_str());

        // 如果有动画，延迟更新状态
        if !reduced_motion {
            let state_clone = Arc::clone(&self.state);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(120));
                if let Ok(mut s) = state_clone.lock() {
                    if *s == OverlayState::Showing {
                        *s = OverlayState::Shown;
                    }
                }
            });
        }

        Ok(())
    }

    pub fn hide(&self, window: &Window, reason: HideReason) -> Result<(), AppError> {
        log::info!("hide: called with reason {:?}", reason);
        let mut state = self
            .state
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))?;

        // 状态机转换
        match *state {
            OverlayState::Hidden => {
                // 已隐藏，幂等
                log::info!("hide: already Hidden, skipping");
                return Ok(());
            }
            OverlayState::Hiding => {
                // 已在隐藏中，忽略
                log::info!("hide: already Hiding, skipping");
                return Ok(());
            }
            OverlayState::Showing => {
                // 取消显示，立即隐藏
                log::info!("hide: state is Showing, will hide immediately");
            }
            OverlayState::Shown => {
                // 正常隐藏
                log::info!("hide: state is Shown, will hide normally");
            }
        }

        // 发送事件
        let _ = window.emit("overlay://hidden", ());

        // 隐藏窗口
        let reduced_motion = self.prefers_reduced_motion_reduced();
        log::info!(
            "hide: reduced_motion={}, reason={:?}",
            reduced_motion,
            reason
        );
        if reduced_motion || reason != HideReason::UserCommand {
            // 立即隐藏（无动画或不需要恢复焦点）
            log::info!("hide: calling window.hide() immediately");
            window.hide()?;
            *state = OverlayState::Hidden;
            log::info!("hide: window hidden, state set to Hidden");

            // 仅 UserCommand 恢复焦点
            if reason == HideReason::UserCommand {
                if let Some(prev) = self.prev_window.lock().ok().and_then(|p| p.clone()) {
                    // spawn_blocking 执行恢复，避免阻塞 async runtime
                    let _ = std::thread::spawn(move || {
                        if restore(&prev, 150).is_ok() {
                            release_stuck_modifiers(&prev);
                        }
                    });
                }
            }
        } else {
            // 带动画隐藏，延迟后恢复焦点
            *state = OverlayState::Hiding;
            let state_clone = Arc::clone(&self.state);
            let window_clone = window.clone();
            let prev_window = self.prev_window.lock().ok().and_then(|p| p.clone());
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(80));
                let _ = window_clone.hide();
                if let Ok(mut s) = state_clone.lock() {
                    if *s == OverlayState::Hiding {
                        *s = OverlayState::Hidden;
                    }
                }

                // 恢复焦点
                if reason == HideReason::UserCommand {
                    if let Some(prev) = prev_window {
                        if restore(&prev, 150).is_ok() {
                            release_stuck_modifiers(&prev);
                        }
                    }
                }
            });
        }

        Ok(())
    }

    /// 粘贴专用：同步隐藏 + 同步恢复焦点
    ///
    /// 不走 `hide` 的 80ms 动画线程，保证后续写剪贴板 / `Ctrl+V` 的时序可控。
    pub fn hide_for_paste(&self, window: &WebviewWindow) -> RestoreOutcome {
        {
            let Ok(mut state) = self.state.lock() else {
                return RestoreOutcome::Failed;
            };
            *state = OverlayState::Hidden;
        }

        let _ = window.emit("overlay://hidden", ());
        if let Err(e) = window.hide() {
            log::warn!("hide_for_paste: window.hide failed: {}", e);
        }

        // 给 DWM 一点时间真正撤下前台窗口，再恢复目标焦点
        std::thread::sleep(std::time::Duration::from_millis(20));

        let prev = self.prev_window.lock().ok().and_then(|p| p.clone());
        let outcome = match prev {
            None => RestoreOutcome::NoTarget,
            Some(prev) => match restore(&prev, 150) {
                Ok(()) => {
                    // 目标线程可能在 Alt 按下时失去焦点，补发卡住的修饰键 KEYUP
                    release_stuck_modifiers(&prev);
                    RestoreOutcome::Restored
                }
                Err(e) => {
                    log::warn!("hide_for_paste: restore failed: {}", e);
                    RestoreOutcome::Failed
                }
            },
        };
        log::info!("hide_for_paste: outcome={:?}", outcome);
        outcome
    }

    /// 降级时重新展示剪贴板面板（不重新 capture，保留 prev_window）
    pub fn resume_clipboard(&self, window: &WebviewWindow) -> Result<(), AppError> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| AppError::Internal("lock poisoned".into()))?;
            *state = OverlayState::Shown;
        }
        *self
            .current_mode
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))? =
            Some(OverlayMode::Clipboard);

        window.show()?;
        let _ = window.set_focus();
        let _ = window.emit("overlay://shown", "clipboard");
        Ok(())
    }

    /// 当前记录的前台窗口（粘贴前做提权预检）
    pub fn prev_window(&self) -> Option<ForegroundWindow> {
        self.prev_window.lock().ok().and_then(|p| p.clone())
    }

    /// 重启粘贴流程的互斥锁（并发激活直接拒绝第二次）
    pub fn paste_guard(&self) -> Arc<Mutex<()>> {
        Arc::clone(&self.paste_lock)
    }

    pub fn resize(&self, window: &Window, height: u32) -> Result<(), AppError> {
        // 夹取 120-560
        let clamped_height = height.clamp(120, 560);

        // 重新计算位置（垂直位置可能需要调整）
        let monitor = get_cursor_monitor()?;
        let (x, y, width, _) = calculate_overlay_position(
            monitor.work_left,
            monitor.work_top,
            monitor.work_width,
            monitor.work_height,
            monitor.scale_factor,
            720,
            clamped_height,
        );

        window.set_size(tauri::LogicalSize::new(width, clamped_height))?;
        window.set_position(tauri::LogicalPosition::new(x, y))?;

        Ok(())
    }

    pub fn on_blur(&self, window: &Window) -> Result<(), AppError> {
        let hide_on_blur = self
            .hide_on_blur
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))?;
        if !*hide_on_blur {
            log::info!("on_blur: hide_on_blur is disabled, skipping");
            return Ok(());
        }

        let state = self
            .state
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))?;
        if *state != OverlayState::Shown {
            // 只在 Shown 状态下响应 blur
            log::info!("on_blur: state is {:?}, not Shown, skipping", *state);
            return Ok(());
        }
        drop(state);

        log::info!("on_blur: calling hide with Blur reason");
        self.hide(window, HideReason::Blur)
    }

    pub fn set_hide_on_blur(&self, value: bool) {
        if let Ok(mut v) = self.hide_on_blur.lock() {
            *v = value;
        }
    }

    pub fn set_prefers_reduced_motion(&self, value: bool) {
        if let Ok(mut v) = self.prefers_reduced_motion.lock() {
            *v = value;
        }
    }

    fn prefers_reduced_motion_reduced(&self) -> bool {
        self.prefers_reduced_motion
            .lock()
            .map(|v| *v)
            .unwrap_or(false)
    }

    pub fn get_state(&self) -> OverlayState {
        self.state
            .lock()
            .map(|s| *s)
            .unwrap_or(OverlayState::Hidden)
    }

    pub fn get_mode(&self) -> Option<OverlayMode> {
        self.current_mode.lock().ok().and_then(|m| *m)
    }
}

#[derive(Debug, Deserialize)]
pub struct ResizePayload {
    pub height: u32,
}

#[derive(Debug, Deserialize)]
pub struct ShowPayload {
    pub mode: String,
}
