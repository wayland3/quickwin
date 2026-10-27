# T-104 mini-design v2

## 改动文件
- `src-tauri/src/platform/focus.rs`（修改）- 添加前台窗口记录与恢复
- `src-tauri/src/services/window.rs`（修改）- 集成焦点恢复，区分 hide 语义
- `src-tauri/Cargo.toml`（修改）- 添加 windows crate features

## 模块交互
```
hotkey → show() → (仅 Hidden→Showing 时) capture_foreground() 存入 OverlayManager
                → window.show()

overlay_hide (带恢复) → hide() → window.hide() → restore(prev) → emit overlay://hidden
on_blur (静默隐藏)   → hide() → window.hide() → emit overlay://hidden (不 restore)
mode 切换            → show(new_mode) → (不 capture，浮层已是前台)
```

## 关键实现点

### 1. 前台窗口记录（capture 时机修复）
```rust
pub struct ForegroundWindow {
    pub hwnd: isize,
    pub pid: u32,
    pub process_name: String,
    pub window_title: String,  // 仅内存中用，不写日志
    pub is_elevated: bool,
    pub is_minimized: bool,
    pub monitor: Option<String>,  // 新增：显示器标识
    pub focus_child: Option<isize>,  // 新增：真实焦点子窗口
}

pub fn capture_foreground() -> Option<ForegroundWindow>  // 改为 Option，失败降级
```
- **调用时机**：仅 `OverlayManager::show()` 中 Hidden→Showing 转换时调用
- **失败降级**：返回 None，OverlayManager 缓存 None，hide 时不恢复
- **提权检测**：TokenElevation 为主
  - `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`
  - `OpenProcessToken(TOKEN_QUERY)`
  - `GetTokenInformation(TokenElevation)`
  - ACCESS_DENIED 仅作兜底信号

### 2. UWP 窗口解析
- 检测窗口类名是否为 `ApplicationFrameHost`
- 如果是，用 `EnumChildWindows` 找子窗口
- 子窗口的进程名才是真实应用名
- 解析失败则使用宿主名，记录 info 日志

### 3. 焦点恢复（RAII 守卫 + 健壮性修复）
```rust
pub struct ThreadInputGuard {
    cur_tid: u32,
    target_tid: u32,  // 改名
}

impl Drop for ThreadInputGuard {
    fn drop(&mut self) {
        // SAFETY: 成对调用
        unsafe { AttachThreadInput(self.cur_tid, self.target_tid, FALSE) };
    }
}

pub fn restore(prev: &ForegroundWindow, timeout_ms: u32) -> Result<(), AppError>
```
- **IsWindow 校验**：第一步检查 `IsWindow(prev.hwnd)`，失效返回降级错误
- **AttachThreadInput 返回值检查**：失败且 ERROR_ACCESS_DENIED → 返回 `AppError::ElevatedTarget`
- **子窗口 SetFocus**：优先 `SetFocus(prev.focus_child)`，拿不到退回顶层
- **执行线程**：`spawn_blocking` 执行，避免阻塞 async runtime
- **超时 150ms**：重试 3 次，每次 50ms

### 4. hide 语义区分
```rust
pub enum HideReason {
    UserCommand,  // overlay_hide 命令，带恢复
    Blur,         // 失焦隐藏，不带恢复
    ModeSwitch,   // 模式切换，不带恢复
}

pub fn hide(&self, window: &Window, reason: HideReason) -> Result<(), AppError>
```
- `UserCommand`：window.hide() → restore(prev) → emit overlay://hidden
- `Blur`/`ModeSwitch`：window.hide() → emit overlay://hidden（不 restore）

### 5. 提权目标处理
- capture_foreground 检测 is_elevated（TokenElevation）
- restore 中 AttachThreadInput 失败且 ERROR_ACCESS_DENIED → 返回 `AppError::ElevatedTarget`
- 前端显示 toast「目标窗口权限较高，已复制，请手动粘贴」

## 错误处理
- 场景 1：GetForegroundWindow 返回空 → 返回 None（降级，不阻断显示）
- 场景 2：OpenProcess 失败 → is_elevated = true，不报错
- 场景 3：IsWindow 失败 → 返回降级错误（非 Internal）
- 场景 4：AttachThreadInput 失败且 ACCESS_DENIED → 返回 `AppError::ElevatedTarget`
- 场景 5：SetForegroundWindow 失败 → 重试 3 次，每次 50ms，仍失败则返回错误
- 场景 6：UWP 解析失败 → 使用宿主名，记录 info 日志

## 测试策略
- 单测：ForegroundWindow 结构体、提权检测逻辑（mock Win32 调用）
- 手测：记事本、VS Code、Chrome、资源管理器、微信中触发浮层后关闭，焦点回到原窗口且光标位置不变；目标为管理员程序时给出提示

## 次要修复
- Cargo.toml 添加 windows features：`Win32_UI_WindowsAndMessaging`、`Win32_System_Threading`、`Win32_Security`
- ThreadInputGuard.fg_tid 改名为 target_tid
