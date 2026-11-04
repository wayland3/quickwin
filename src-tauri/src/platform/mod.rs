// 平台抽象层：按操作系统条件编译
// Windows: Win32 API (windows crate)
// Linux: X11/Wayland (arboard, enigo, image crate)

#[cfg(target_os = "windows")]
pub mod apps;
#[cfg(target_os = "windows")]
pub mod clipboard;
#[cfg(target_os = "windows")]
pub mod focus;
#[cfg(target_os = "windows")]
pub mod imaging;
#[cfg(target_os = "windows")]
pub mod input;

#[cfg(target_os = "linux")]
pub mod apps;
#[cfg(target_os = "linux")]
pub mod clipboard;
#[cfg(target_os = "linux")]
pub mod focus;
#[cfg(target_os = "linux")]
pub mod imaging;
#[cfg(target_os = "linux")]
pub mod input;

// Win32 平台层：剪贴板、热键、焦点、应用列表、文件索引
// 所有 unsafe 调用必须在此模块内，并有 SAFETY 注释
