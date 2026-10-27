# Win32 平台笔记

本文是实现约束与踩坑记录。所有代码必须遵守 `AGENTS.md` 第 2 节与本文的 SAFETY 约定。

## 1. 总则

- 所有 `unsafe` 与 Win32 调用只出现在 `src-tauri/src/platform/`。
- 每个 `unsafe` 块上方写 `// SAFETY:`：说明句柄来源、生命周期、失败路径。
- 句柄/内存用 RAII 守卫：`ClipboardGuard`（Drop 时 `CloseClipboard`）、`GlobalMemGuard`（Drop 时 `GlobalUnlock`）。
- 错误一律映射为 `AppError`，不把 `HRESULT` / `GetLastError` 数字抛给 UI；日志里保留 code。
- 阻塞调用不得出现在 `async` 上下文中；用 `spawn_blocking` 或专用线程。

## 2. 前台窗口与焦点

### 2.1 记录

```rust
// SAFETY: GetForegroundWindow 无参数，返回可能为空，需判空。
let hwnd = unsafe { GetForegroundWindow() };
let mut pid = 0u32;
// SAFETY: hwnd 有效；pid 为栈上可变引用。
unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
```

还要记录：进程可执行名（`QueryFullProcessImageNameW`，需 `PROCESS_QUERY_LIMITED_INFORMATION`）、窗口标题（仅内存中用，不写日志）、是否提权。

UWP/商店应用：前台窗口类名常为 `ApplicationFrameHost`。用 `GetWindowThreadProcessId` 拿到的是宿主进程，需再取真实 AUMID（`GetApplicationUserModelId`）或用 `EnumChildWindows` 找真实窗口。解析失败就显示宿主名，不影响功能。

### 2.2 恢复（AttachThreadInput 模式）

Windows 有前台锁（foreground lock），直接 `SetForegroundWindow` 经常静默失败：

```text
cur_tid  = GetCurrentThreadId()
fg_tid   = GetWindowThreadProcessId(prev_hwnd)
AttachThreadInput(cur_tid, fg_tid, true)
  ShowWindow(prev_hwnd, SW_SHOW)       // 已最小化时
  SetForegroundWindow(prev_hwnd)
  SetFocus(prev_hwnd)                  // 部分应用需要
AttachThreadInput(cur_tid, fg_tid, false)   // 必须在 finally 语义里执行
```

要求：

- `AttachThreadInput` 必须成对；用守卫保证异常/提前返回时也会 detach。
- 恢复后校验：`GetForegroundWindow() == prev_hwnd`，否则视为失败。
- 总预算 150ms，超出就放弃并走降级提示。
- 不要修改系统 `SPI_SETFOREGROUNDLOCKTIMEOUT`（影响全局，用户会感知）。

### 2.3 提权目标（UIPI）

- 目标进程完整性级别高于本进程时，`SendInput` 会被静默忽略，`OpenProcess` 也可能拒绝。
- 判定方式：`OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` 失败（`ERROR_ACCESS_DENIED`）→ 视为提权；或读 token 的 `TokenElevation`。
- 处理：返回 `AppError::ElevatedTarget`，UI 提示「目标窗口权限较高，已复制，请手动粘贴」。**不要**尝试用 `UIAccess`、注入、驱动绕过。
- 自身以管理员运行时不受此限制，但会导致自启、拖拽等体验问题；v1 默认不提权运行，也不提供「始终以管理员运行」。

## 3. 输入模拟（SendInput）

```rust
// SAFETY: INPUT 为 POD，mem::zeroed 合法；cbSize 必须等于 size_of::<INPUT>()。
let mut input: INPUT = unsafe { std::mem::zeroed() };
input.r#type = INPUT_KEYBOARD;
input.Anonymous.ki.wVk = VIRTUAL_KEY(0);
input.Anonymous.ki.wScan = ch as u16;
input.Anonymous.ki.dwFlags = KEYEVENTF_UNICODE;   // 松开时再加 KEYEVENTF_KEYUP
```

### 3.1 文本

- 用 `KEYEVENTF_UNICODE` 逐 UTF-16 code unit 发送；**代理对（emoji）要拆成两个 `u16` 分别发**。
- 不要用 `VkKeyScan` 处理中文/emoji。
- 每个字符 down + up；长文本每 50 字符 `sleep(1ms)`，避免部分应用丢字。
- 发送前先补发 `Ctrl/Shift/Alt/Win` 的 KEYUP，防止用户按着修饰键导致状态卡住。

### 3.2 组合键（Ctrl+C / Ctrl+V）

```
send KEYUP(alt/win/shift)          // 清理残留
send KEYDOWN(ctrl)
send KEYDOWN(v)
send KEYUP(v)
send KEYUP(ctrl)
```

- 按键间隔 8–15ms，过短在部分 Electron/Java 应用里丢事件。
- 不要用 `keybd_event`（已废弃，且不参与 `SendInput` 的序列化）。
- `SendInput` 返回发送数量，少于预期要重试一次，再失败返回 `AppError::Internal`。

### 3.3 已知不响应的情况

- 提权窗口（见 2.3）。
- 部分游戏（Raw Input / DirectInput）、部分远程桌面与虚拟机窗口。
- UAC 安全桌面。
- 光标不在可编辑控件时（无法检测，只能尽力发送）。

## 4. 剪贴板

### 4.1 监听

- v1 用 `GetClipboardSequenceNumber()` 每 250ms 轮询：无变化时零成本，不需要窗口与消息循环。
- 变化后延迟 30ms 再读，避免源应用还没写完（大图/长文本常见）。
- `OpenClipboard(Some(hwnd))` 失败（被 Office/浏览器占用）时重试 5 次 × 20ms；仍失败跳过本次并计数，**不要**强杀占用进程。
- 读取完成后立刻 `CloseClipboard`，期间不做 IO、不 `await`。
- 若将来需要事件驱动，再引入 `AddClipboardFormatListener` + 隐藏窗口，属于独立 ADR。

### 4.2 格式

| 格式 | 说明 |
|---|---|
| `CF_UNICODETEXT` | 主文本，必须 |
| `CF_HTML` | 含固定头部，`StartHTML/EndHTML/StartFragment/EndFragment` 是**字节偏移**，10 位零填充；解析时按偏移截取 fragment |
| `CF_RTF` | 直接是 RTF 文本 |
| `CF_DIB` / `CF_DIBV5` | 位图；优先找注册格式 `PNG`，没有则 DIB 转 PNG 再存 |
| `CF_HDROP` | 文件列表，`DragQueryFileW` 逐个取 |
| `HTML Format` / `Rich Text Format` | 注册格式名，需 `RegisterClipboardFormatW` 获取 id |

### 4.3 敏感内容（已移除）

> **注意**：此功能已于 2026-10-27 由用户要求移除。

~~以下任一命中则**整条丢弃**，不记录类型、不计数：~~

~~| 格式名 | 判定 |
|---|---|
| `ExcludeClipboardContentFromMonitorProcessing` | 存在即排除 |
| `CanIncludeInClipboardHistory` | `DWORD == 0` 排除 |
| `CanUploadToCloudClipboard` | `DWORD == 0` 时标记 `sensitive`（本地仍可存，但不入 FTS） |~~

~~密码管理器（KeePassXC、1Password、Bitwarden）会设置这些标记。这是安全底线，不是可选项。~~

### 4.4 快照与还原

取词需要「临时借走剪贴板」再还回去：

1. `EnumClipboardFormats` 收集所有格式 id。
2. 对每个格式 `GetClipboardData` → `GlobalLock` → 复制字节 → `GlobalUnlock`。
3. 记录 `GetClipboardSequenceNumber`。
4. 发送 `Ctrl+C`，读文本。
5. 还原：`EmptyClipboard` → 对每个可还原格式 `GlobalAlloc(GMEM_MOVEABLE)` + 拷贝 + `SetClipboardData`。

已知边界：

- 部分格式（延迟渲染、私有格式、OLE 对象）无法完整还原；跳过并记录数量。
- 还原会再次改变序列号，监听线程必须通过「抑制窗口 + hash」跳过，否则历史里会出现一条本不该存在的记录。
- 还原失败时**不要**清空剪贴板，保留当前内容更安全，并提示用户「剪贴板可能已变化」。
- 图片尽量按 PNG 还原；DIB 还原在部分应用会丢透明度。

### 4.5 写入

- `OpenClipboard` → `EmptyClipboard` → 按「富格式先、纯文本后」顺序 `SetClipboardData` → `CloseClipboard`。
- `SetClipboardData` 成功后内存所有权归系统，**不要再 `GlobalFree`**。
- 写入前把内容转为 `GMEM_MOVEABLE`；大图 >8MB 直接放弃写入并返回 `TooLarge`。
- 写入完成后不要立刻校验内容（有些格式是延迟渲染，读不到不代表失败）。

## 5. 全局热键

- 使用 `tauri-plugin-global-shortcut`（底层 `RegisterHotKey`）。
- `Alt+Space` 是系统窗口菜单的默认键，且常被 PowerToys Run 占用；注册可能失败。
- 注册结果必须检查：失败时保留旧热键、emit `hotkey://failed`、托盘提示、设置页标红。
- 改绑顺序：**先注册新键成功，再注销旧键**；失败则完全不动。
- 进程退出前注销全部热键；异常退出由系统回收，不要依赖。
- `MOD_NOREPEAT` 需要显式设置（插件通常已处理）；按住热键不应连续触发。
- 不要用 `WH_KEYBOARD_LL` 低级钩子做备用方案：杀软敏感、性能与稳定性风险高，v1 明确不做。
- 双击修饰键（如双击 Ctrl）触发属于 v2 需求，需要钩子，届时单独评估。

## 6. DPI 与多显示器

- 应用必须声明 Per-Monitor V2 DPI 感知（Tauri 默认通常已处理；若窗口在 150% 缩放下模糊，检查 manifest / `SetProcessDpiAwarenessContext`，需在创建窗口前调用）。
- 定位流程：

```text
GetCursorPos(pt)                       // 物理像素
MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST)
GetMonitorInfoW → rcWork               // 物理像素，已排除任务栏
physical_x = rcWork.left + (rcWork.width - window_physical_w) / 2
physical_y = rcWork.top + (rcWork.height * 0.2)
```

- Tauri 的 `set_position` 接收逻辑像素：`logical = physical / scale_factor`。混合 DPI 多屏时**先取目标显示器的 scale_factor**，不能用主屏的。
- 窗口高度变化后要重新计算垂直位置，避免底部超出工作区。
- 竖屏（宽 < 高）时宽度取 `min(720, rcWork.width - 32)`。

## 7. 应用与文件

### 7.1 快捷方式

- `.lnk` 解析用 COM `IShellLinkW`（`CoInitializeEx` 在专用线程，注意 STA/MTA）或成熟的 lnk 解析 crate（新增依赖需 ADR）。
- 优先取 `GetPath`，空则跳过（可能是 UWP 或 MSI 广告快捷方式）。
- 图标：`ExtractIconExW` 或 `SHGetFileInfoW`，缓存到 `%APPDATA%\com.quickwin.app\icons\<hash>.png`，避免每次读 exe。

### 7.2 启动

- 普通程序：`ShellExecuteW`（走文件关联、UAC 清单、工作目录正确）。
- 管理员：`ShellExecuteW` + `runas`，会弹 UAC；用户取消时静默返回。
- UWP：`IApplicationActivationManager::ActivateApplication(aumid, ...)`。
- 打开文件所在位置：`explorer.exe /select,"<path>"`。
- 不要用 `CreateProcessW` 直接起 exe（会丢工作目录、参数转义和 UAC 清单）。

### 7.3 文件列表

- `CF_HDROP`：`DragQueryFileW(hdrop, 0xFFFFFFFF, ...)` 取数量，再逐个取路径。
- 路径可能是虚拟文件（Outlook 附件等），`GetFileAttributesW` 失败时标记 `exists=false`，UI 显示「源文件不可用」。
- 长路径（>260）用 `\\?\` 前缀；同时应用 manifest 开启 `longPathAware`。
- 打开前校验存在性，避免 `ShellExecute` 弹系统错误框。

## 8. 安全与合规

- 不注入其他进程、不写 `AppInit_DLLs`、不做键盘钩子、不读其他进程内存。
- 开机自启走 Tauri autostart（注册表 `Run` 键由插件管理），不手写注册表。
- 剪贴板内容不落日志、不进崩溃上报；数据库本地明文，并在 UI 明确告知。
- 提权相关：默认以当前用户权限运行；提示用户「以管理员运行 QuickWin 可支持高权限窗口」，但不默认开启。
- 安装包未签名时 SmartScreen 会提示，属预期；发布前评估签名。

## 9. 其他坑位

| 现象 | 原因 | 处理 |
|---|---|---|
| 浮层在全屏游戏上不显示 | 独占全屏 | 提示切无边框窗口；不做 DirectX 覆盖 |
| 中文输入法下取词异常 | 输入法组合态吞掉 Ctrl+C | 先发送 `Esc` 或等待 50ms 再复制（实验性，失败率可接受） |
| 剪贴板读到的文本带 `\0` | `CF_UNICODETEXT` 双 null 结尾 | 按第一个 `\0` 截断 |
| 粘贴后多出换行 | 源文本含 `\r\n`，目标控件转换 | 保留原样，不做隐式转换 |
| 图片粘贴后变黑/透明丢失 | DIB 无 alpha | 统一转 PNG 存储与写回 |
| 浮层显示后原窗口最小化 | 焦点切换触发 | 显示前记录 `IsIconic`，恢复时 `ShowWindow(SW_RESTORE)` |
| 高对比度主题下文字看不清 | 硬编码颜色 | 用系统主题监听，至少支持跟随系统暗/亮 |

## 10. 手动测试清单

每个涉及 `platform/` 的任务都要跑，并在任务备注里记录环境（Windows 版本 / 缩放 / 是否多屏）。

**基础**

- [ ] 记事本、VS Code、Chrome 地址栏、Word、微信：`Alt+X` 取词正确
- [ ] 同上 5 个应用：替换结果正确，无残留字符
- [ ] 同上 5 个应用：`Alt+V` 回车粘贴正确
- [ ] 管理员运行的记事本：提示「权限较高」，剪贴板可用
- [ ] 输入法（微软拼音）开启时取词与粘贴
- [ ] 选中 emoji + 中文 + 多行文本

**焦点**

- [ ] 触发前窗口是最小化状态，关闭浮层后恢复
- [ ] 触发前窗口在另一个显示器，关闭后焦点回去
- [ ] 连续开关 20 次，焦点每次都正确

**剪贴板**

- [ ] 复制 1MB 文本（截断标记）
- [ ] 复制 10MB 图片（仅缩略图）
- [ ] 资源管理器复制 3 个文件
- [ ] ~~从 KeePassXC 复制密码（不入库）~~：已移除（2026-10-27 用户要求）
- [ ] Excel 复制单元格（OpenClipboard 占用场景）
- [ ] 复制后立刻按 `Alt+V`（30ms 延迟是否足够）

**显示**

- [ ] 100% / 150% / 200% 缩放下浮层清晰且位置正确
- [ ] 竖屏 + 双屏
- [ ] 全屏视频、全屏浏览器（F11）
- [ ] 任务栏在顶部/左侧时定位正确

**生命周期**

- [ ] 二次启动不产生新实例
- [ ] 退出后热键全部释放（再按无反应）
- [ ] 强制结束进程后重启，数据完整、无锁库
