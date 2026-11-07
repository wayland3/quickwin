// 输入模拟层（SendInput）
//
// # 阻塞警告
// 本模块所有函数都会阻塞当前线程，禁止在 async 上下文直接调用。
// 后续接入时需 spawn_blocking 或专用线程。

use crate::error::AppError;

/// 发送 Unicode 文本（含 emoji/代理对）
///
/// # 阻塞警告
/// 此函数会阻塞当前线程，禁止在 async 上下文直接调用。
pub fn send_text(text: &str) -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        send_text_windows(text)
    }

    #[cfg(target_os = "linux")]
    {
        send_text_linux(text)
    }
}

/// 发送 Ctrl+C
pub fn send_copy() -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        send_copy_windows()
    }

    #[cfg(target_os = "linux")]
    {
        send_copy_linux()
    }
}

/// 发送粘贴快捷键
///
/// `process_name` 用于检测目标窗口是否为终端，以选择合适的粘贴快捷键。
/// Linux 终端通常使用 `Ctrl+Shift+V`，其他应用使用 `Ctrl+V`。
pub fn send_paste(process_name: Option<&str>) -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        let _ = process_name; // Windows 终端一般支持 Ctrl+V，暂不需要特殊处理
        send_paste_windows()
    }

    #[cfg(target_os = "linux")]
    {
        send_paste_linux(process_name)
    }
}

// ============================================================
// Windows 实现
// ============================================================
#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN,
        VK_SHIFT,
    };

    pub fn send_text_windows(text: &str) -> Result<(), AppError> {
        // 清理修饰键
        clear_modifiers()?;

        // 转为 UTF-16
        let utf16: Vec<u16> = text.encode_utf16().collect();

        // 分批发送，每 50 字符 sleep 1ms
        for (i, chunk) in utf16.chunks(50).enumerate() {
            for &code_unit in chunk {
                let inputs = [
                    make_key_input(0, code_unit, KEYEVENTF_UNICODE),
                    make_key_input(0, code_unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
                ];
                send_inputs(&inputs)?;
            }

            // 每 50 字符后 sleep（最后一批不 sleep）
            if i < utf16.len() / 50 {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }

        Ok(())
    }

    pub fn send_copy_windows() -> Result<(), AppError> {
        clear_modifiers()?;

        let inputs = [
            make_key_input(VK_CONTROL.0, 0, KEYBD_EVENT_FLAGS(0)),
            make_key_input(VIRTUAL_KEY(0x43).0, 0, KEYBD_EVENT_FLAGS(0)), // 'C'
            make_key_input(VIRTUAL_KEY(0x43).0, 0, KEYEVENTF_KEYUP),
            make_key_input(VK_CONTROL.0, 0, KEYEVENTF_KEYUP),
        ];

        // 按键间隔 10ms
        for input in &inputs {
            send_inputs(&[*input])?;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        Ok(())
    }

    pub fn send_paste_windows() -> Result<(), AppError> {
        clear_modifiers()?;

        let inputs = [
            make_key_input(VK_CONTROL.0, 0, KEYBD_EVENT_FLAGS(0)),
            make_key_input(VIRTUAL_KEY(0x56).0, 0, KEYBD_EVENT_FLAGS(0)), // 'V'
            make_key_input(VIRTUAL_KEY(0x56).0, 0, KEYEVENTF_KEYUP),
            make_key_input(VK_CONTROL.0, 0, KEYEVENTF_KEYUP),
        ];

        // 按键间隔 10ms
        for input in &inputs {
            send_inputs(&[*input])?;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        Ok(())
    }

    fn clear_modifiers() -> Result<(), AppError> {
        let modifiers = [VK_MENU, VK_LWIN, VK_RWIN, VK_SHIFT, VK_CONTROL];

        for vk in modifiers {
            // SAFETY: GetAsyncKeyState 是安全的 Win32 调用
            let state = unsafe { GetAsyncKeyState(vk.0 as i32) };
            // 检查最低位（按键是否按下）- state 是 i16，0x8000 转为 i16 是 -32768
            if state & (-32768i16) != 0 {
                let input = make_key_input(vk.0, 0, KEYEVENTF_KEYUP);
                send_inputs(&[input])?;
            }
        }

        Ok(())
    }

    fn make_key_input(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn send_inputs(inputs: &[INPUT]) -> Result<u32, AppError> {
        // SAFETY: inputs 为有效 INPUT 数组
        let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };

        if sent == 0 {
            return Err(AppError::ElevatedTarget);
        }

        if sent < inputs.len() as u32 {
            return Err(AppError::Internal(format!(
                "SendInput partial: sent {} of {}",
                sent,
                inputs.len()
            )));
        }

        Ok(sent)
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
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    use std::sync::Mutex;

    static ENIGO: Mutex<Option<Enigo>> = Mutex::new(None);

    fn get_enigo() -> Result<std::sync::MutexGuard<'static, Option<Enigo>>, AppError> {
        let mut guard = ENIGO
            .lock()
            .map_err(|_| AppError::Internal("enigo mutex poisoned".into()))?;

        if guard.is_none() {
            *guard = Some(
                Enigo::new(&Settings::default())
                    .map_err(|e| AppError::Internal(format!("enigo init failed: {}", e)))?,
            );
        }

        Ok(guard)
    }

    pub fn send_text_linux(text: &str) -> Result<(), AppError> {
        let mut guard = get_enigo()?;
        let enigo = guard
            .as_mut()
            .ok_or(AppError::Internal("enigo not initialized".into()))?;

        enigo
            .text(text)
            .map_err(|e| AppError::Internal(format!("send_text failed: {}", e)))?;

        Ok(())
    }

    pub fn send_copy_linux() -> Result<(), AppError> {
        send_key_combo(Key::Control, Key::Unicode('c'))
    }

    pub fn send_paste_linux(process_name: Option<&str>) -> Result<(), AppError> {
        // Linux 终端通常使用 Ctrl+Shift+V 粘贴，而非 Ctrl+V
        let is_term = is_terminal(process_name);
        log::info!(
            "send_paste_linux: process_name={:?}, is_terminal={}",
            process_name,
            is_term
        );
        if is_term {
            send_key_combo_with_two_modifiers(Key::Control, Key::Shift, Key::Unicode('v'))
        } else {
            send_key_combo(Key::Control, Key::Unicode('v'))
        }
    }

    /// 检测进程名是否为终端模拟器
    fn is_terminal(process_name: Option<&str>) -> bool {
        let Some(name) = process_name else {
            return false;
        };
        let name_lower = name.to_lowercase();
        // 常见 Linux 终端模拟器进程名
        matches!(
            name_lower.as_str(),
            "gnome-terminal"
                | "gnome-terminal-" // gnome-terminal-server
                | "konsole"
                | "alacritty"
                | "kitty"
                | "xterm"
                | "uxterm"
                | "wezterm"
                | "wezterm-gui"
                | "terminator"
                | "tilix"
                | "xfce4-terminal"
                | "lxterminal"
                | "mate-terminal"
                | "st" // suckless terminal
                | "foot"
                | "sakura"
                | "terminology"
                | "guake"
                | "yakuake"
        )
    }

    fn send_key_combo_with_two_modifiers(
        modifier1: Key,
        modifier2: Key,
        key: Key,
    ) -> Result<(), AppError> {
        let mut guard = get_enigo()?;
        let enigo = guard
            .as_mut()
            .ok_or(AppError::Internal("enigo not initialized".into()))?;

        enigo
            .key(modifier1, Direction::Press)
            .map_err(|e| AppError::Internal(format!("modifier1 press failed: {}", e)))?;

        enigo
            .key(modifier2, Direction::Press)
            .map_err(|e| AppError::Internal(format!("modifier2 press failed: {}", e)))?;

        enigo
            .key(key, Direction::Click)
            .map_err(|e| AppError::Internal(format!("key click failed: {}", e)))?;

        enigo
            .key(modifier2, Direction::Release)
            .map_err(|e| AppError::Internal(format!("modifier2 release failed: {}", e)))?;

        enigo
            .key(modifier1, Direction::Release)
            .map_err(|e| AppError::Internal(format!("modifier1 release failed: {}", e)))?;

        Ok(())
    }

    fn send_key_combo(modifier: Key, key: Key) -> Result<(), AppError> {
        let mut guard = get_enigo()?;
        let enigo = guard
            .as_mut()
            .ok_or(AppError::Internal("enigo not initialized".into()))?;

        enigo
            .key(modifier, Direction::Press)
            .map_err(|e| AppError::Internal(format!("modifier press failed: {}", e)))?;

        enigo
            .key(key, Direction::Click)
            .map_err(|e| AppError::Internal(format!("key click failed: {}", e)))?;

        enigo
            .key(modifier, Direction::Release)
            .map_err(|e| AppError::Internal(format!("modifier release failed: {}", e)))?;

        Ok(())
    }
}

#[cfg(target_os = "linux")]
use linux_impl::*;

#[cfg(test)]
mod tests {
    #[test]
    #[allow(non_snake_case)]
    fn UTF16代理对拆分正确() {
        // emoji: 😀 (U+1F600)
        let text = "😀";
        let utf16: Vec<u16> = text.encode_utf16().collect();
        assert_eq!(utf16.len(), 2); // 代理对拆成两个 u16
        assert_eq!(utf16[0], 0xD83D);
        assert_eq!(utf16[1], 0xDE00);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 普通字符UTF16长度正确() {
        let text = "hello";
        let utf16: Vec<u16> = text.encode_utf16().collect();
        assert_eq!(utf16.len(), 5);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 中文字符UTF16长度正确() {
        let text = "你好";
        let utf16: Vec<u16> = text.encode_utf16().collect();
        assert_eq!(utf16.len(), 2); // 中文不在代理对范围
    }
}
