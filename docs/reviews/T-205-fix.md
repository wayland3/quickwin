# T-205 修复：粘贴焦点与修饰键卡死

**实现模型**：deepseek-v4.1-flash-economy
**评审模型**：GLM-5.3（固定）
**评审级别**：L2（涉及 `platform/` Win32 边界与焦点/UIPI 行为）
**评审日期**：2026-10-30
**评审轮次**：1
**结论**：PASS

## 问题现象（2026-10-29 真机）

1. 剪贴板选中后按 `Enter` 不粘贴，只提示「已复制，请手动粘贴」
2. 之后点击其他应用（如钉钉关闭按钮）第一下无效
3. 启动器/剪贴板面板按 `Esc` 或点击外部不退出

## 根因

1. **焦点恢复误判**：`restore` 无条件先 `AttachThreadInput`，失败即返回 `ElevatedTarget`，
   导致即使本进程与目标同为提权也无法恢复；日志显示 `prev_elevated=true, self_elevated=true`
   仍被判失败
2. **修饰键卡在目标线程**：`Alt+V` 的 keydown 发给目标线程、keyup 发给浮层线程；切回焦点后
   目标线程仍认为 Alt 按下，进入菜单模式吞掉第一次点击
3. **失焦隐藏从未接线**：`OverlayManager::on_blur` 存在但没有任何 Tauri 窗口事件调用它

## 修复

- `platform/focus.rs`：`restore` 改为先直接 `SetForegroundWindow`，失败再用 `AttachThreadInput`
  兜底且附加失败不中止；新增 `release_stuck_modifiers`（附加目标线程读 `GetKeyboardState`，
  只对仍按下的 Alt/Ctrl/Shift/Win 补发 KEYUP）；新增 `current_process_elevated`
- `services/paste.rs`：目标提权但本进程也提权时允许粘贴；恢复后等 30ms 再发 `Ctrl+V`
- `services/window.rs`：`hide` 与 `hide_for_paste` 恢复焦点成功后清理卡住的修饰键
- `lib.rs`：`Builder::on_window_event` 监听 overlay `Focused(false)` → `on_blur` 隐藏
- 诊断日志：`overlay show` / `hide_for_paste outcome` / `paste` 结果 / 失焦，均只含 id 与布尔/结果

## 验证

- 用户真机确认：`Enter` 直接粘贴成功、`Esc` 与点击外部均退出、粘贴后点击其他应用恢复正常
- 日志证据：`hide_for_paste: outcome=Restored`、`paste: sent ctrl+v`、`overlay lost focus, hiding`
- `npm.cmd run verify` 全绿：131 Rust passed / 4 ignored、53 前端 passed、clippy `-D warnings`

## NONBLOCK（登记遗留）

1. **`Showing` 120ms 动画窗口期内的失焦被忽略**（`services/window.rs` `on_blur` 只认 `Shown`）
   - 极端情况下浮层可能短暂滞留
   - 处理：登记遗留，后续放宽为 `Showing | Shown`
2. **`hide` 的恢复在线程中不持 `paste_lock`**（`services/window.rs`）
   - 150ms 内重新呼出时，旧恢复线程可能抢回焦点
   - 处理：登记遗留，后续恢复前重读状态或与粘贴锁互斥
3. **KEYUP 为全局注入**（`platform/focus.rs` `release_stuck_modifiers`）
   - 恢复瞬间用户切走时会误释放其他应用的修饰键
   - 处理：登记遗留，后续注入前校验 `GetForegroundWindow` 仍为目标
