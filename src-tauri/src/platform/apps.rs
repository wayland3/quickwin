//! 应用索引采集的 Win32 出口：快捷方式解析、App Paths 注册表、UWP 枚举。
//!
//! 约定：
//! - 所有 COM / Win32 调用集中在 `platform`，上层只拿纯 Rust 结构。
//! - 不记录快捷方式路径、应用名或注册表值到日志（`AGENTS.md` §10）。
//! - 单项失败降级为跳过，由调用方统计数量；只有整批无法开始时才返回错误。

use crate::error::AppError;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::core::{Interface, GUID, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HANDLE, HWND, RPC_E_CHANGED_MODE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IBindCtx, IPersistFile,
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, REG_EXPAND_SZ, REG_SZ,
};
use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
/// `KF_FLAG_DEFAULT` 来自该模块（值为 0）
use windows::Win32::UI::Shell::KF_FLAG_DEFAULT;
use windows::Win32::UI::Shell::{
    FOLDERID_AppsFolder, IShellFolder, IShellItem2, IShellLinkW, SHCreateItemWithParent,
    SHGetDesktopFolder, SHGetKnownFolderIDList, SHCONTF_FOLDERS, SHCONTF_NONFOLDERS,
    SIGDN_NORMALDISPLAY,
};

/// `CLSID_ShellLink`：快捷方式 COM 对象
const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x0002_1401_0000_0000_c000_0000_0000_0046);

/// `PKEY_AppUserModel_ID`（UWP 的 AUMID）
const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c_2855_9f79_4b39_a8d0_e1d4_2de1_d5f3),
    pid: 5,
};

/// `SLGP_RAWPATH`：按快捷方式里保存的原始路径取，避免触发网络/卷解析
const SLGP_RAWPATH: u32 = 4;

/// COM 初始化守卫
///
/// 只在专用阻塞线程上构造；`S_OK` / `S_FALSE` 都表示本线程持有一份引用计数，
/// 退出时配对 `CoUninitialize`；`RPC_E_CHANGED_MODE` 表示线程已被其他 apartment
/// 初始化，此时可直接调用 shell COM，但**不能**反初始化。
struct ComApartment {
    should_uninit: bool,
}

impl ComApartment {
    fn new() -> Result<Self, AppError> {
        // SAFETY: pvReserved 必须为 null；在未初始化的阻塞线程上首次调用
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr.is_ok() {
            Ok(Self {
                should_uninit: true,
            })
        } else if hr == RPC_E_CHANGED_MODE {
            Ok(Self {
                should_uninit: false,
            })
        } else {
            Err(AppError::Internal(format!(
                "COM 初始化失败: 0x{:08X}",
                hr.0
            )))
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.should_uninit {
            // SAFETY: 与 new() 中成功的 CoInitializeEx 一一配对，只执行一次
            unsafe { CoUninitialize() };
        }
    }
}

/// 解析后的快捷方式
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedShortcut {
    /// 快捷方式自身的路径（用于取显示名；不会写入日志）
    pub link: PathBuf,
    /// 可执行文件路径（原始路径，未做环境变量展开）
    pub target: PathBuf,
    /// 启动参数
    pub args: Option<String>,
    /// 图标位置，`path` 或 `path,index`
    pub icon_path: Option<String>,
}

/// 批量解析结果
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShortcutScan {
    pub resolved: Vec<ResolvedShortcut>,
    /// 解析失败或被跳过的快捷方式数量（不记录路径本身）
    pub failed: usize,
}

/// 批量解析 `.lnk`，内部初始化 COM
pub fn resolve_shortcuts(paths: &[PathBuf]) -> Result<ShortcutScan, AppError> {
    let _com = ComApartment::new()?;
    let mut scan = ShortcutScan::default();
    for path in paths {
        match resolve_shortcut(path) {
            Some(resolved) => scan.resolved.push(resolved),
            None => scan.failed += 1,
        }
    }
    Ok(scan)
}

fn resolve_shortcut(link: &Path) -> Option<ResolvedShortcut> {
    // SAFETY: CLSID_ShellLink 是进程内 COM 组件；punkOuter 传 None
    let shell_link: IShellLinkW =
        unsafe { CoCreateInstance(&CLSID_SHELL_LINK, None, CLSCTX_INPROC_SERVER) }.ok()?;
    let persist: IPersistFile = shell_link.cast().ok()?;

    // 保持宽字符缓冲存活到 Load 调用结束
    let link_wide = wide_path(link);
    // SAFETY: 缓冲区以 NUL 结尾且在调用期间有效
    unsafe { persist.Load(PCWSTR::from_raw(link_wide.as_ptr()), STGM_READ) }.ok()?;

    let mut path_buf = vec![0u16; 2048];
    // SAFETY: pfd 允许为 null；缓冲区长度与调用匹配
    unsafe { shell_link.GetPath(&mut path_buf, std::ptr::null_mut(), SLGP_RAWPATH) }.ok()?;
    let target = string_from_wide(&path_buf)?;

    let mut args_buf = vec![0u16; 1024];
    let args = if unsafe { shell_link.GetArguments(&mut args_buf) }.is_ok() {
        string_from_wide(&args_buf).filter(|value| !value.trim().is_empty())
    } else {
        None
    };

    let mut icon_buf = vec![0u16; 1024];
    let mut icon_index = 0i32;
    let icon_path = if unsafe { shell_link.GetIconLocation(&mut icon_buf, &mut icon_index) }.is_ok()
    {
        string_from_wide(&icon_buf).map(|icon| {
            if icon_index == 0 {
                icon
            } else {
                format!("{icon},{icon_index}")
            }
        })
    } else {
        None
    };

    Some(ResolvedShortcut {
        link: link.to_path_buf(),
        target: PathBuf::from(target),
        args,
        icon_path,
    })
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// 从调用方缓冲区（NUL 结尾）取字符串
fn string_from_wide(buffer: &[u16]) -> Option<String> {
    let end = buffer.iter().position(|&unit| unit == 0)?;
    let text = String::from_utf16_lossy(&buffer[..end]);
    (!text.is_empty()).then_some(text)
}

/// `App Paths` 条目
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPathEntry {
    /// 显示名（键名去掉 `.exe`）
    pub name: String,
    /// 默认值指向的可执行文件路径
    pub target: PathBuf,
}

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY: 句柄由本模块打开，所有权已转移到 Guard，只关闭一次
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

/// 读取全部 `App Paths` 条目；单个子键读取失败时跳过并计数
pub fn list_app_paths() -> Vec<AppPathEntry> {
    let mut entries = Vec::new();
    let mut failed = 0usize;
    for (subkey, root) in app_paths_subkeys() {
        match read_app_paths_root(root, subkey) {
            Ok(mut found) => entries.append(&mut found),
            Err(_) => failed += 1,
        }
    }
    if failed > 0 {
        log::warn!("App Paths 子键读取失败: count={}", failed);
    }
    entries
}

fn app_paths_subkeys() -> [(&'static str, HKEY); 3] {
    [
        (
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths",
            HKEY_LOCAL_MACHINE,
        ),
        (
            "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\App Paths",
            HKEY_LOCAL_MACHINE,
        ),
        (
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths",
            HKEY_CURRENT_USER,
        ),
    ]
}

fn read_app_paths_root(root: HKEY, subkey: &str) -> Result<Vec<AppPathEntry>, AppError> {
    let mut key = HKEY::default();
    let subkey_wide = wide_str(subkey);
    // SAFETY: 子键缓冲以 NUL 结尾且在调用期间有效
    let status = unsafe {
        RegOpenKeyExW(
            root,
            PCWSTR::from_raw(subkey_wide.as_ptr()),
            0,
            KEY_READ,
            &mut key,
        )
    };
    if status != ERROR_SUCCESS {
        // 子键不存在（常见于精简系统）视为空，不视为失败
        return Ok(Vec::new());
    }
    let key = RegistryKey(key);

    let mut entries = Vec::new();
    let mut index = 0u32;
    loop {
        let mut name_buf = [0u16; 256];
        let mut name_len = name_buf.len() as u32;
        // SAFETY: name_buf 长度与 name_len 匹配；lpClass 等可选参数传空
        let status = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                PWSTR::from_raw(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                PWSTR::null(),
                None,
                None,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status != ERROR_SUCCESS {
            return Err(AppError::Internal(format!(
                "枚举 App Paths 键失败: 0x{:08X}",
                status.0
            )));
        }
        index += 1;

        let raw_name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
        if let Some(target) = read_default_value(key.0, &raw_name) {
            entries.push(AppPathEntry {
                name: strip_exe_suffix(&raw_name),
                target,
            });
        }
    }

    Ok(entries)
}

fn strip_exe_suffix(name: &str) -> String {
    if name.len() >= 4 && name[name.len() - 4..].eq_ignore_ascii_case(".exe") {
        name[..name.len() - 4].to_string()
    } else {
        name.to_string()
    }
}

fn read_default_value(parent: HKEY, child_name: &str) -> Option<PathBuf> {
    let mut child = HKEY::default();
    let child_wide = wide_str(child_name);
    // SAFETY: parent 是已打开且存活的键；子键名以 NUL 结尾
    let status = unsafe {
        RegOpenKeyExW(
            parent,
            PCWSTR::from_raw(child_wide.as_ptr()),
            0,
            KEY_READ,
            &mut child,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let child = RegistryKey(child);
    read_default_string(child.0)
}

fn read_default_string(key: HKEY) -> Option<PathBuf> {
    let mut value_type = REG_SZ;
    let mut len = 0u32;
    // SAFETY: 第一次只查询长度（数据指针为空）；默认值名为空字符串
    let status = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR::null(),
            None,
            Some(&mut value_type),
            None,
            Some(&mut len),
        )
    };
    if status != ERROR_SUCCESS || len == 0 {
        return None;
    }

    let mut buffer = vec![0u8; len as usize];
    // SAFETY: buffer 长度与 len 匹配，写入范围合法
    let status = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR::null(),
            None,
            Some(&mut value_type),
            Some(buffer.as_mut_ptr()),
            Some(&mut len),
        )
    };
    if status != ERROR_SUCCESS || (value_type != REG_SZ && value_type != REG_EXPAND_SZ) {
        return None;
    }

    let (pairs, _) = buffer.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    string_from_wide(&units).map(PathBuf::from)
}

fn wide_str(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// UWP 应用
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UwpApp {
    /// 系统显示名（真实名称，非包名）
    pub name: String,
    /// `AppUserModelID`
    pub aumid: String,
}

/// 枚举 `shell:AppsFolder` 中的 UWP 应用；整体失败返回错误，由调用方降级
pub fn list_uwp_apps() -> Result<Vec<UwpApp>, AppError> {
    let _com = ComApartment::new()?;

    // SAFETY: AppsFolder 是系统已知文件夹；htoken 传空表示当前用户
    let folder_pidl = unsafe {
        SHGetKnownFolderIDList(
            &FOLDERID_AppsFolder,
            KF_FLAG_DEFAULT.0 as u32,
            None::<&HANDLE>,
        )
    }
    .map_err(|error| AppError::Internal(format!("定位 AppsFolder 失败: {}", error)))?;
    // SAFETY: 桌面 Shell 文件夹为系统对象，引用计数归调用方
    let desktop = unsafe { SHGetDesktopFolder() }
        .map_err(|error| AppError::Internal(format!("打开桌面 Shell 失败: {}", error)))?;

    // SAFETY: PIDL 由 SHGetKnownFolderIDList 分配、绑定期间有效；pbc 允许为空
    let folder = unsafe { desktop.BindToObject::<_, IShellFolder>(folder_pidl, None::<&IBindCtx>) };
    // SAFETY: 绑定完成后不再使用该 PIDL，必须由调用方释放
    unsafe { CoTaskMemFree(Some(folder_pidl as *const core::ffi::c_void)) };
    let folder = folder
        .map_err(|error| AppError::Internal(format!("绑定 AppsFolder 到 Shell 失败: {}", error)))?;

    let mut enumerator = None;
    // SAFETY: 窗口句柄传空表示无 owner；枚举标志为文件夹 + 非文件夹
    let hr = unsafe {
        folder.EnumObjects(
            HWND::default(),
            (SHCONTF_FOLDERS.0 | SHCONTF_NONFOLDERS.0) as u32,
            &mut enumerator,
        )
    };
    if hr.is_err() {
        return Err(AppError::Internal(format!(
            "枚举 AppsFolder 失败: 0x{:08X}",
            hr.0
        )));
    }
    let Some(enumerator) = enumerator else {
        return Ok(Vec::new());
    };

    let mut apps = Vec::new();
    loop {
        let mut item_pidl: [*mut windows::Win32::UI::Shell::Common::ITEMIDLIST; 1] =
            [std::ptr::null_mut()];
        let mut fetched = 0u32;
        // SAFETY: 单元素输出切片在调用期间有效；PIDL 由枚举器分配，归调用方释放
        let hr = unsafe { enumerator.Next(&mut item_pidl, Some(&mut fetched)) };
        let pidl = item_pidl[0];
        if hr.is_err() || fetched == 0 || pidl.is_null() {
            if !pidl.is_null() {
                // SAFETY: 指针可能非空但未计入 fetched，同样由调用方释放
                unsafe { CoTaskMemFree(Some(pidl as *const core::ffi::c_void)) };
            }
            break;
        }

        // SAFETY: PIDL 在本轮内保持有效；父文件夹为已绑定的 AppsFolder
        let item = unsafe { SHCreateItemWithParent::<_, IShellItem2>(None, &folder, pidl) };
        // SAFETY: 无论 GetUIObjectOf 成功与否，PIDL 已不再被使用
        unsafe { CoTaskMemFree(Some(pidl as *const core::ffi::c_void)) };

        let Ok(item) = item else {
            continue;
        };

        // SAFETY: 属性键为公开常量；返回值由 COM 分配，take_pwstr 负责释放
        let aumid = match unsafe { item.GetString(&PKEY_APP_USER_MODEL_ID) } {
            Ok(ptr) => take_pwstr(ptr),
            Err(_) => None,
        };
        let Some(aumid) = aumid else {
            // 没有 AUMID 的项多为分组标题或分隔符，跳过
            continue;
        };

        // SAFETY: SIGDN_NORMALDISPLAY 返回用户可见名称；返回值同样需要释放
        let name = match unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY) } {
            Ok(ptr) => take_pwstr(ptr),
            Err(_) => None,
        };
        let Some(name) = name else {
            continue;
        };

        apps.push(UwpApp { name, aumid });
    }

    Ok(apps)
}

/// 取走 COM 分配的 `PWSTR` 并释放
fn take_pwstr(ptr: PWSTR) -> Option<String> {
    if ptr.0.is_null() {
        return None;
    }
    // SAFETY: COM 返回的宽字符串以 NUL 结尾，且在释放前一直有效
    let text = unsafe { ptr.to_string() }.ok();
    // SAFETY: 指针由 COM 用 CoTaskMemAlloc 分配，必须由调用方用 CoTaskMemFree 释放
    unsafe { CoTaskMemFree(Some(ptr.0 as *const core::ffi::c_void)) };
    text.filter(|value| !value.is_empty())
}
