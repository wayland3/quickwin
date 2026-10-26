pub mod logging;
pub mod tray;

// 业务逻辑层：storage、search、ranking、hotkey、config、tray
// 不直接调用 Win32，通过 platform 模块
