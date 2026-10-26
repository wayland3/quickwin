---
name: Tauri Win32
description: 在 QuickWin 中实现或修改 Win32 相关能力（剪贴板、SendInput、前台窗口/焦点、全局热键、DPI、应用启动）时使用，含封装模板、坑位与手动测试清单。
---

# Win32 实现规范

适用目录：`src-tauri/src/platform/`。任何 `windows` crate 调用都必须先读本技能与 `docs/WIN32-NOTES.md`。

## 开始前检查

- [ ] 该能力是否已有封装？先搜 `platform/`，禁止重复实现。
- [ ] 是否必须用 Win32？能用 Tauri 插件/标准库解决的，优先用现成的。
- [ ] 调用发生在哪个线程？是否会在 `async` 中阻塞？
- [ ] 失败时的用户可见行为是什么？（中文提示 + 降级动作）
- [ ] 是否会读取剪贴板内容/窗口标题？若是，确认不写日志。

## 封装约定

```rust
// platform/mod.rs
pub mod clipboard;
pub mod focus;
pub mod input;

// 对外只暴露安全接口
pub fn paste_text(text: &str) -> Result<(), AppError>;
pub fn foreground_window() -> Result<WindowInfo, AppError>;
```

规则：

1. **句柄不跨线程**：`HWND` 只作为值传递（`isize`），不在多线程共享 `HANDLE`。
2. **RAII**：`ClipboardGuard` 在 Drop 中 `CloseClipboard`；`GlobalMemGuard` 在 Drop 中 `GlobalUnlock`。
3. **错误映射**：`GetLastError` → `AppError`，保留 code 到日志，message 用中文。
4. **`unsafe` 注释**：

```rust
// SAFETY: hwnd 来自 GetForegroundWindow，可能为空但已在上一行判空；
// 调用期间窗口句柄不会被本模块销毁。
unsafe { ShowWindow(hwnd, SW_SHOW) };
```

5. **超时预算**：焦点恢复 150ms、剪贴板打开 5×20ms、取词 400ms。超过就降级，不无限重试。

## 模板：剪贴板写入（多格式）

```text
write(item):
  if item.size > MAX: return TooLarge
  guard = open_clipboard_with_retry(5, 20ms)?   // Drop 时 Close
  EmptyClipboard()
  if let Some(html) = item.html { set(CF_HTML, html_bytes)? }
  if let Some(rtf)  = item.rtf  { set(CF_RTF, rtf_bytes)? }
  if let Some(png)  = item.png  { set(register("PNG"), png)? }
  if let Some(files)= item.files{ set(CF_HDROP, hdrop_bytes)? }
  set(CF_UNICODETEXT, utf16_with_double_nul(item.text)?)?   // 最后写纯文本
  // 成功后内存所有权归系统，禁止 GlobalFree
```

要点：

- `SetClipboardData` 前必须 `EmptyClipboard`，且当前线程持有剪贴板。
- 文本结尾需要**双 NUL**（UTF-16）。
- `CF_HTML` 的头部偏移是字节数，10 位零填充；不要用字符数。
- 写入失败中途不要留下半套格式：失败就 `EmptyClipboard` 并返回错误。

## 模板：快照与还原

```text
snapshot():
  guard = open_clipboard()?
  for fmt in enum_formats():
      handle = GetClipboardData(fmt)
      ptr = GlobalLock(handle)          // 可能失败，跳过
      bytes = copy(ptr, GlobalSize(handle))
      GlobalUnlock(handle)
      out.push((fmt, bytes))
  return out

restore(snapshot):
  guard = open_clipboard()?
  EmptyClipboard()
  for (fmt, bytes) in snapshot:
      if !restorable(fmt): skipped += 1; continue
      h = GlobalAlloc(GMEM_MOVEABLE, len)
      copy bytes; GlobalUnlock
      SetClipboardData(fmt, h)         // 失败则 GlobalFree(h)
  log(skipped)                          // 只记数量，不记内容
```

还原后序列号会变化，监听线程必须跳过（hash + 1.5s 抑制窗口）。

## 模板：焦点恢复

```text
restore_focus(prev_hwnd, budget=150ms):
  if prev_hwnd.is_null(): return Err(NoTarget)
  if is_elevated(prev_hwnd): return Err(ElevatedTarget)
  cur = GetCurrentThreadId()
  fg  = GetWindowThreadProcessId(prev_hwnd)
  ok  = false
  attach(cur, fg, true)
  defer attach(cur, fg, false)
  if IsIconic(prev_hwnd): ShowWindow(prev_hwnd, SW_RESTORE)
  SetForegroundWindow(prev_hwnd)
  SetFocus(prev_hwnd)
  wait_until(|| GetForegroundWindow() == prev_hwnd, budget)
```

失败返回 `AppError::Internal("focus")`，调用方降级为「已复制，请手动粘贴」。

## 模板：Unicode 输入

```text
send_text(s):
  release_stuck_modifiers()                  // ctrl/shift/alt/win 全部 KEYUP
  for unit in s.encode_utf16():
      send(KEYEVENTF_UNICODE, unit)
      send(KEYEVENTF_UNICODE | KEYEVENTF_KEYUP, unit)
      if every 50 units: sleep(1ms)
  if SendInput 返回不足: 重试一次
```

emoji 是代理对，必须逐 `u16` 发送；不要按 `char` 发送。

## 高频坑位

| 坑 | 症状 | 正确做法 |
|---|---|---|
| 忘记 `EmptyClipboard` | 新旧格式混合，粘贴内容错 | 写入前先清空 |
| `CF_HTML` 用字符偏移 | 粘贴出乱码/截断 | 用字节偏移，10 位零填充 |
| `GlobalFree` 已交给系统的内存 | 崩溃/堆损坏 | `SetClipboardData` 成功后不再释放 |
| `OpenClipboard(None)` | 部分场景失败 | 传一个有效窗口句柄（浮层或隐藏窗口） |
| 在 `async` 里 `OpenClipboard` | 卡住整个 runtime | `spawn_blocking` 或专用线程 |
| 直接 `SetForegroundWindow` | 静默失败 | `AttachThreadInput` + 校验 |
| 不清理修饰键 | 粘贴变成 `Ctrl+Alt+V` | 先发 KEYUP |
| 发送 `char` 而非 `u16` | emoji 变问号 | `encode_utf16` |
| 用物理坐标调 Tauri | 高 DPI 下位置偏移 | 除以目标显示器 `scale_factor` |
| 忘记 `MOD_NOREPEAT` | 按住热键连续触发 | 注册时显式设置 |
| 忽略 `CanIncludeInClipboardHistory` | 密码进历史 | 读取前先查敏感标记 |
| 取词不还原剪贴板 | 用户剪贴板被覆盖 | 快照 + 还原 + 抑制监听 |
| 把窗口标题写日志 | 隐私泄露 | 只记进程名与长度 |

## 验证要求

1. `cargo clippy --all-targets -- -D warnings` 通过，无新增 `unwrap`。
2. 按 `docs/WIN32-NOTES.md` 第 10 节跑**对应分组**，不是全跑；但至少覆盖：目标应用 3 个、管理员窗口 1 个、多屏/缩放 1 项。
3. 记录环境：Windows 版本号、缩放比例、是否多屏、输入法状态。
4. 失败场景要主动构造：剪贴板被占用、无选中文本、焦点窗口已关闭。

## 升级路径（不要现在做）

- 需要更低延迟 → 评估 `AddClipboardFormatListener`（ADR-0006 升级条件）。
- 需要更强取词 → 先试 UI Automation `TextPattern`，失败再回退复制（ADR-0007）。
- 需要双击修饰键 → 需要低级钩子，必须先写 ADR 并获得用户同意（ADR-0012 明确当前不做）。
