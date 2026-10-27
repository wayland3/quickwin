# T-102 mini-design v2

## 改动文件
- `src-tauri/src/services/window.rs`（新增）- 浮层窗口管理（状态机）
- `src-tauri/src/platform/focus.rs`（新增）- 获取鼠标所在显示器 + DPI
- `src-tauri/src/platform/mod.rs`（修改）- 导出 focus 模块
- `src-tauri/src/commands/mod.rs`（修改）- 添加 overlay_show/hide/resize 命令
- `src/App.tsx`（修改）- 监听 overlay://shown/hidden 事件
- `src/lib/keys.ts`（新增）- Esc 键处理
- `docs/DECISIONS.md`（修改）- 补充 ADR：services/window.rs 新增

## 模块交互
```
commands::overlay_* → services::window (状态机) → platform::focus (目标显示器 DPI/位置)
                                               → tauri Window API (显示/隐藏/定位)
                                               → emit overlay://shown/hidden
```

## 关键实现点

### 1. DPI 与位置计算（纯函数，可单测）
```rust
fn calculate_position(
    rc_work: RECT,           // 目标显示器工作区（物理像素）
    target_dpi: u32,         // 目标显示器 DPI（GetDpiForMonitor）
    window_width: u32,       // 窗口逻辑宽度（720）
    window_height: u32,      // 窗口逻辑高度（120-560）
) -> (i32, i32, u32, u32)   // 返回逻辑坐标 (x, y, width, height)
```
- 用 `MonitorFromPoint(GetCursorPos)` 获取目标显示器句柄
- 用 `GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI)` 获取目标 DPI
- `scale_factor = target_dpi / 96.0`
- `logical_work = rc_work / scale_factor`
- `x = logical_work.left + (logical_work.width - window_width) / 2`
- `y = logical_work.top + logical_work.height * 0.2`
- 竖屏：`window_width = min(720, logical_work.width - 32)`

### 2. 焦点管理
- **显示前**：`GetForegroundWindow` 记录前台 HWND，缓存到 `services::window`
- **显示**：`window.show()` + `window.set_focus()`（常规路径）
- **set_focus 失败**：只记日志，不影响显示（前台锁下常静默失败）
- **隐藏后**：预留 `focus.restore(prev_hwnd)` 调用点（T-104 实现，本任务空实现 + TODO）
- **全屏检测**：删除事前拦截，直接 show；独占全屏提示降级为事后兜底（需求池）

### 3. 状态机
```rust
enum OverlayState {
    Hidden,
    Showing,  // 动画中
    Shown,
    Hiding,   // 动画中
}
```
- 所有入口先过状态机：重复 hide 幂等忽略
- hide 引发的 blur 事件被状态机过滤（Hiding 状态下忽略 blur）
- show 请求取消进行中的 Hiding 并复位 opacity
- 状态转换 emit 对应事件：Showing → `overlay://shown`，Hidden → `overlay://hidden`

### 4. 失焦隐藏
- 服务内布尔开关 `hide_on_blur`（默认 true，T-107 接设置）
- 监听 `blur` 事件，仅在 `Shown` 状态且 `hide_on_blur == true` 时触发 hide

### 5. 动画与减少动画
- CSS transition：显示 120ms ease-out（opacity 0→1 + translateY 4px→0）
- 隐藏 80ms ease-in（opacity 1→0）
- 检测系统动画设置：`SystemParametersInfo(SPI_GETCLIENTAREAANIMATION)`
- 命中减少动画 → 跳过动画等待，立即显隐
- 前端：`prefers-reduced-motion` 媒体查询同步

### 6. 动态高度
- 前端通过 `overlay_resize` 命令调整窗口高度
- 夹取 120–560（逻辑像素）
- 调整后重新计算垂直位置避免底部超出工作区

## 错误处理
- 场景 1：获取显示器失败 → 回退到主显示器，记录 warn
- 场景 2：窗口操作失败 → 返回 AppError::Internal，落日志 + 托盘提示（浮层不可见时无处渲染）

## 测试策略
- 单测：DPI 换算纯函数、位置计算纯函数、状态机转换
- 手测：双屏不同缩放（100%/150%）、全屏视频上按热键、失焦隐藏、连续开关 50 次（脚本驱动 + 手动清单）

## 事件契约
- `overlay://shown`：浮层显示后（载荷 `{ mode }`）
- `overlay://hidden`：浮层隐藏后（无载荷）
- `overlay://resize`：前端请求调整高度（载荷 `{ height }`）

## 次要修复
- `services/window.rs` 是新增模块，补充 ADR 到 DECISIONS.md
- 错误路径：浮层不可见时错误落日志 + 托盘提示
