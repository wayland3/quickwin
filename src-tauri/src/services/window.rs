// 浮层窗口管理（状态机）

use crate::error::AppError;
use crate::platform::focus::{calculate_overlay_position, get_cursor_monitor};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Window};

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
    Actions,
}

impl OverlayMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            OverlayMode::Launcher => "launcher",
            OverlayMode::Clipboard => "clipboard",
            OverlayMode::Actions => "actions",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "launcher" => Some(OverlayMode::Launcher),
            "clipboard" => Some(OverlayMode::Clipboard),
            "actions" => Some(OverlayMode::Actions),
            _ => None,
        }
    }
}

pub struct OverlayManager {
    state: Arc<Mutex<OverlayState>>,
    current_mode: Arc<Mutex<Option<OverlayMode>>>,
    hide_on_blur: Arc<Mutex<bool>>,
    prefers_reduced_motion: Arc<Mutex<bool>>,
}

impl OverlayManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(OverlayState::Hidden)),
            current_mode: Arc::new(Mutex::new(None)),
            hide_on_blur: Arc::new(Mutex::new(true)),
            prefers_reduced_motion: Arc::new(Mutex::new(false)),
        }
    }

    pub fn show(&self, window: &Window, mode: OverlayMode) -> Result<(), AppError> {
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
                    return self.hide(window);
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
                // 正常显示
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

    pub fn hide(&self, window: &Window) -> Result<(), AppError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))?;

        // 状态机转换
        match *state {
            OverlayState::Hidden => {
                // 已隐藏，幂等
                return Ok(());
            }
            OverlayState::Hiding => {
                // 已在隐藏中，忽略
                return Ok(());
            }
            OverlayState::Showing => {
                // 取消显示，立即隐藏
            }
            OverlayState::Shown => {
                // 正常隐藏
            }
        }

        // 发送事件
        let _ = window.emit("overlay://hidden", ());

        // 隐藏窗口
        let reduced_motion = self.prefers_reduced_motion_reduced();
        if reduced_motion {
            window.hide()?;
            *state = OverlayState::Hidden;
        } else {
            *state = OverlayState::Hiding;
            let state_clone = Arc::clone(&self.state);
            let window_clone = window.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(80));
                let _ = window_clone.hide();
                if let Ok(mut s) = state_clone.lock() {
                    if *s == OverlayState::Hiding {
                        *s = OverlayState::Hidden;
                    }
                }
            });
        }

        Ok(())
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
            return Ok(());
        }

        let state = self
            .state
            .lock()
            .map_err(|_| AppError::Internal("lock poisoned".into()))?;
        if *state != OverlayState::Shown {
            // 只在 Shown 状态下响应 blur
            return Ok(());
        }
        drop(state);

        self.hide(window)
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
