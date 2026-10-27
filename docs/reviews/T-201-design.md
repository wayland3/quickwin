# T-201 mini-design v2

## 改动文件
- `src-tauri/src/services/clipboard.rs`（新增）- 剪贴板监听服务
- `src-tauri/src/platform/clipboard.rs`（新增）- 剪贴板 Win32 封装
- `src-tauri/src/platform/mod.rs`（修改）- 导出 clipboard 模块
- `src-tauri/src/services/mod.rs`（修改）- 导出 clipboard 模块
- `src-tauri/src/lib.rs`（修改）- 启动监听线程

## 模块交互
```
lib.rs::setup → services::clipboard::start_monitor → platform::clipboard::get_sequence_number
                                                    → platform::clipboard::open/close
                                                    → emit clipboard://changed
```

## 关键实现点

### 1. platform/clipboard.rs（Win32 封装）
```rust
pub fn get_sequence_number() -> Result<u32, AppError>
pub fn open_clipboard(hwnd: HWND) -> Result<ClipboardGuard, AppError>

pub struct ClipboardGuard {
    hwnd: HWND,
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: hwnd 有效
        unsafe { CloseClipboard() };
    }
}
```
- `get_sequence_number`：调用 `GetClipboardSequenceNumber`
- `open_clipboard`：调用 `OpenClipboard`，失败返回错误
- `ClipboardGuard`：RAII 守卫，drop 时自动调用 `CloseClipboard`

### 2. services/clipboard.rs（监听服务）
```rust
pub struct ClipboardMonitor {
    stop_flag: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ClipboardMonitor {
    pub fn start(app: AppHandle) -> Result<Self, AppError>
    pub fn stop(&self)
}

impl Drop for ClipboardMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}
```

**线程生命周期管理**：
- 使用 `Arc<AtomicBool>` 作为取消标志
- `start` 时 spawn 线程，保存 `JoinHandle`
- `stop` 时设置 `stop_flag = true`，并 `join()` 线程
- `Drop` 时自动调用 `stop`，确保应用退出时线程优雅关闭

**序列号溢出处理**：
```rust
let last_seq = state.last_sequence_number;
let current_seq = get_sequence_number()?;
// 使用 wrapping_sub 处理回绕
if current_seq.wrapping_sub(last_seq) != 0 {
    // 触发事件
    state.last_sequence_number = current_seq;
}
```

**动态重试策略**：
- 检测到变化后立即尝试读取
- 失败则 10ms 后重试
- 再失败则 50ms 后重试
- 总窗口 100-200ms，若仍失败则记录警告并跳过
- 取消固定 30ms 延迟，改为"立即尝试 + 失败后退避"

### 3. 线程模型
- 使用 `std::thread::spawn` 启动独立线程
- 线程内循环检查 `stop_flag`，为 false 时继续轮询
- 轮询间隔 250ms（`std::thread::sleep`）
- 通过 `app.emit` 发送事件到前端
- 线程退出时 `join()` 确保资源释放

### 4. 错误处理
- `get_sequence_number` 失败：记录日志，继续轮询
- `open_clipboard` 失败：立即重试 → 10ms 后重试 → 50ms 后重试，总窗口 100-200ms
- 仍失败则记录警告并跳过本次，继续下一轮轮询
- 读取期间不得 `await`（同步阻塞）

## 测试策略

### 单测
- `ClipboardGuard` 生命周期（drop 时自动关闭）
- 序列号溢出处理（wrapping_sub）
- 取消标志检查（stop_flag）

### 手测入口
创建 `tests/clipboard_manual.rs`（`#[ignore]`），用 `cargo test -- --ignored` 执行：

```rust
#[test]
#[ignore]
fn 手测_连续复制20次() {
    // 启动监听，连续复制 20 次（间隔 < 30ms）
    // 验证：全部捕获且无重复事件
}

#[test]
#[ignore]
fn 手测_Excel占用剪贴板() {
    // 打开 Excel 并复制大表格（占用数秒）
    // 验证：监听线程不卡死，CPU ≈ 0%，最终能捕获
}
```

### 手测清单
- 环境记录：Windows 版本、缩放、多屏
- 应用：记事本、Chrome、资源管理器、Excel
- 内容：文本、图片、文件列表
- 失败场景：Excel 占用剪贴板、Office 复制中
- 快速复制：间隔 < 30ms 连续复制 20 次

## 次要修复
- Cargo.toml 添加 windows features：`Win32_System_DataExchange`、`Win32_System_Memory`
