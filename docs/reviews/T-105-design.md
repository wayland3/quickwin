# T-105 mini-design v2

## 改动文件
- `src-tauri/src/platform/input.rs`（新增）- 输入模拟层
- `src-tauri/src/platform/mod.rs`（修改）- 导出 input 模块

**注意**：本任务是纯 platform 层封装，不新增 command。后续 T-108 会接入 command 层。

## 模块交互
```
（本任务无 command 入口）
后续 T-108：commands::input_* → platform::input::send_text/send_copy/send_paste
```

## 关键实现点

### 1. send_text（阻塞函数，禁止在 async 中直接调用）
```rust
/// 发送 Unicode 文本（含 emoji/代理对）
/// 
/// # 阻塞警告
/// 此函数会阻塞当前线程（每 50 字符 sleep 1ms），禁止在 async 上下文直接调用。
/// 后续接入时需 spawn_blocking 或专用线程。
pub fn send_text(text: &str) -> Result<(), AppError>
```
- 清理修饰键：发送 Ctrl/Shift/Alt/Win 的 KEYUP
- 逐 UTF-16 code unit 发送：
  - 普通字符：KEYEVENTF_UNICODE down + up
  - 代理对（emoji）：拆成两个 u16 分别发
- 每 50 字符 sleep(1ms)，避免丢字
- SendInput 失败处理：
  - 返回 0（零注入）→ 检查 UIPI（复用 T-104 提权检测），命中返回 `AppError::ElevatedTarget`
  - 部分注入（0 < ret < len）→ 不重试，返回 `AppError::Internal` 并记录已注入数量
  - 零注入且非 UIPI → 重试一次

### 2. send_copy / send_paste（收窄 API 面）
```rust
pub fn send_copy() -> Result<(), AppError>
pub fn send_paste() -> Result<(), AppError>
```
- 内部实现：清理修饰键 → 发送 Ctrl+C / Ctrl+V
- 不暴露通用 send_combo，避免任意按键注入能力

### 3. 修饰键清理
```rust
fn clear_modifiers() -> Result<(), AppError>
```
- 先 `GetAsyncKeyState` 查询，确认按下才发 KEYUP
- 发送 VK_MENU (Alt)、VK_LWIN/VK_RWIN、VK_SHIFT、VK_CONTROL 的 KEYUP

### 4. INPUT 结构构造
```rust
fn make_key_input(vk: u16, scan: u16, flags: u32) -> INPUT
```
- cbSize = size_of::<INPUT>()
- type = INPUT_KEYBOARD
- ki.wVk = vk
- ki.wScan = scan
- ki.dwFlags = flags
- ki.time = 0
- ki.dwExtraInfo = 0

### 5. SendInput 调用
```rust
fn send_inputs(inputs: &[INPUT]) -> Result<u32, AppError>
```
- 调用 SendInput
- 返回实际注入数量（由调用方判断失败语义）

## 错误处理
- 场景 1：SendInput 返回 0 + UIPI → 返回 `AppError::ElevatedTarget`
- 场景 2：SendInput 返回 0 + 非 UIPI → 重试一次，仍失败返回 `AppError::Internal`
- 场景 3：部分注入（0 < ret < len）→ 返回 `AppError::Internal`，记录已注入数量

## 测试策略

### 单测
- combo 发送逻辑（内部函数）
- 代理对拆分逻辑（UTF-16 编码）

### 手测入口
创建 `tests/input_manual.rs`（`#[ignore]` 标记），用 `cargo test -- --ignored` 执行：

```rust
#[test]
#[ignore]
fn 手测_发送中文英文emoji() {
    // 打开记事本，运行此测试
    // 验证：中文、英文、emoji 均正确显示
}

#[test]
#[ignore]
fn 手测_连续发送100次() {
    // 发送固定文本 ×100
    // 保存后比对字符数与内容
}
```

### 手测清单
- 环境记录：Windows 版本、缩放、多屏、输入法状态（中/英）
- 应用：记事本、Chrome 地址栏、VS Code 编辑器
- 内容：中文、英文、emoji
- 失败场景：管理员窗口（应提示权限较高）、焦点窗口已关闭
- IME 状态：中文输入法开/关两种状态都测

## 次要修复
- Cargo.toml 的 `Win32_UI_Input_KeyboardAndMouse` feature 已存在（T-104 已加），无需修改
