# T-206 mini-design

## 背景

用户反馈：无法把热键设为 `Win + 空格`（Win+Space）。

实测结论（Linux/X11，本机）：

| 环节 | 结论 |
|------|------|
| 前端 `eventToAccelerator` | `metaKey → Super`，产出 `Super+Space` ✅ |
| 后端 `global-hotkey` 解析 | `SUPER → Modifiers::SUPER` ✅ |
| X11 `grab_key(Super+Space)` | **成功**（WM 抓的是 Super 单键，组合键不冲突）✅ |
| `global-hotkey` 事件回调 | 实测收到 `Pressed`/`Released` ✅ |
| **前端录制器 blur** | **❌ 根因：Win 键触发桌面 Activities，窗口失焦 → `handleBlur` 直接退出录制态** |

所以后端能力齐备，**唯一阻塞点是前端录制器在 Win 键导致失焦时中止录制**。

## 改动文件

- `src/components/HotkeyRecorder.tsx`（修改）— 录制态容忍 Super 导致的失焦；修饰键松开时兜底退出
- `src/lib/keys.ts`（修改）— 新增 `isModifierHeld(event)`，判定修饰键是否按住
- `src-tauri/src/services/hotkey.rs`（修改）— 新增 `HotkeyManager::probe()` 与 `is_bound()`
- `src-tauri/src/commands/mod.rs`（修改）— 新增 `hotkey_probe` command（**同步** fn）
- `src-tauri/src/error.rs`（修改）— 新增 `HotkeyProbeFailed(String)` + 错误码 `hotkey.probe_failed`
- `src/lib/ipc.ts`（修改）— 新增 `hotkeyProbe()`
- `src/windows/SettingsPanel.tsx`（修改）— 热键页显示实测检测结果
- `src/lib/ipc.contract.test.ts`（修改）— 契约测试补 `HotkeyProbeResult`
- `src/lib/contract/HotkeyProbeResult.keys.json`（新增）— 契约 fixture
- `src/windows/SettingsPanel.test.tsx`（新增）— 设置页检测 UI 测试

## 模块交互

```
commands::hotkey_probe → services::hotkey::HotkeyManager::probe → global_hotkey::GlobalHotKeyManager
```

探测用**独立**的 `GlobalHotKeyManager`，与运行时绑定（走 `tauri-plugin-global-shortcut`）
是两套实例，探测后立即 `unregister`，不长期占用、不干扰运行时。

**但**底层 OS 的全局热键表是同一份 —— 运行时已注册的组合键，探测时同样返回
`AlreadyRegistered`。因此必须先用 `is_bound()` 识别「本应用自己占用的键」，
否则用户会看到自己正在用的热键被标红为「被占用」。

## 错误处理

结论与错误二分：

| 情况 | 结果 | 理由 |
|------|------|------|
| 组合键空闲 | `Ok(Available)` | 结论 |
| 被其他程序占用 | `Ok(Occupied)` | 结论 |
| 本应用自己已绑定 | `Ok(Available, "本应用已绑定")` | 结论 |
| accelerator 格式非法 | `Ok(Invalid)` | 结论 |
| 探测环境故障（X11 断连等） | `Err(HotkeyProbeFailed)` | **失败** |

前端 `hotkey.probe_failed` 走白名单展示后端中文原因（类似 `launcher.launch_failed` 的处理）。

## 设计决策

1. **command 用同步 `fn`**：探测底层要同步注册/注销全局热键，
   `async fn` 里做阻塞调用违反 `docs/CODING-STANDARDS.md`。Tauri 会把同步 command
   放到专用线程执行。
2. **新增依赖 `global-hotkey = "0.8"`**：见 `docs/DECISIONS.md` ADR-0017。
   本就是插件的传递依赖，版本一致；插件把类型化错误降级成了字符串，无法区分占用与故障。

## 测试策略

- 单测（Rust）：`parse_shortcut` 格式校验；`probe` 对非法/空值返回 `Invalid`；
  `is_bound` 的 trim 归一化；`probe` 对自身绑定返回 `Available`；结果字段集合契约
- 单测（前端 `keys.ts`）：`isModifierHeld` 判定
- 单测（前端 `HotkeyRecorder`）：Win 键导致 blur 后仍能录到 `Super+Space`；
  修饰键松开后退出录制态；无修饰键时 blur 仍退出；保存失败后松开修饰键退出且保留错误提示
- 单测（前端 `SettingsPanel`）：自动探测两个热键；可用/被占用/探测失败三种展示；手动重测
- 契约测试：`HotkeyProbeResult` 字段集合前端 fixture 与 Rust 序列化一致
- 手测（Linux 本机 X11）：录 `Super+Space` → 保存 → 按该组合键能呼出浮层

## 设计自检

- [x] 新增依赖？→ **是**，`global-hotkey = "0.8"`（插件的传递依赖，版本一致）→ 需 ADR（ADR-0017，已获用户确认）
- [x] 改目录结构？→ 否
- [x] 改 IPC 契约？→ 是，新增 `hotkey_probe` → 已同步 `ipc.ts` + 契约测试
- [x] 涉及 Win32 边界？→ 否（`RegisterHotKey` / `grab_key` 均由 `global-hotkey` 封装）
- [x] 改验收标准？→ 否

**结论**：设计自检通过；新增依赖部分已写 ADR-0017 并经用户确认。
