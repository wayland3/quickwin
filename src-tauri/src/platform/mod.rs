pub mod apps;
pub mod clipboard;
pub mod focus;
pub mod imaging;
pub mod input;

// Win32 平台层：剪贴板、热键、焦点、应用列表、文件索引
// 所有 unsafe 调用必须在此模块内，并有 SAFETY 注释
