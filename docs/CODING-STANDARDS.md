# 编码规范

本文档定义 Quick 的编码规范，写代码前必读。

## 1. Rust

- Edition 2021，`rustfmt` 默认配置；`clippy -D warnings` 必须通过。
- 错误：`thiserror` 定义 `AppError`，对前端暴露稳定的 `code`（如 `clipboard.busy`）+ 中文 `message`；不把 Win32 HRESULT 直接抛给 UI。
- 异步：Tauri command 用 `async fn`，但**禁止在 `async` 中做阻塞 Win32 调用或长事务**；阻塞工作丢到 `spawn_blocking` 或专用线程。
- SQLite：单写线程 + `tokio::sync::mpsc`；读用短连接或连接池。任何 `await` 期间不得持有数据库锁或已打开的剪贴板句柄。
- `unsafe`：只允许在 `platform/` 内，且每个 `unsafe` 块上方写 `// SAFETY:` 说明前置条件与生命周期。
- 资源：HWND/HANDLE/`GlobalLock` 用 RAII 守卫封装，禁止裸 `let h = OpenClipboard(...)` 后分支提前 return。

## 2. TypeScript / React

- `strict: true`，禁止 `any`（确需时用 `unknown` + 类型守卫）。
- 组件只负责渲染与交互，派生逻辑放 `lib/`，纯函数必须有测试。
- 所有 `invoke` 走 `lib/ipc.ts`，入参/返回值类型与 Rust `models.rs` 手工对齐；字段变更必须同步两侧并更新测试。
- 不引入 UI 组件库（Tailwind + 自建组件），保证浮层体积与启动速度。

## 3. 命名与文案

- 任务 ID：`T-<里程碑><序号>`，如 `T-206`。
- Command：`snake_case`（`clipboard_list`）；事件：`domain://event`（`clipboard://changed`）。
- 数据库表：`snake_case` 单数（`clipboard_item`）。
- 面向用户：简体中文，不使用「您」；按钮动词开头（「粘贴」「删除」）。

## 4. 数据与迁移

- 库文件：`quick/quick.db`；附件：`blobs\<xx>\<hash>.<ext>`。
- `PRAGMA journal_mode=WAL`、`synchronous=NORMAL`、`busy_timeout=5000`、`foreign_keys=ON`。
- 版本：`PRAGMA user_version`，迁移函数数组顺序执行，单次迁移必须在事务内。
- 迁移测试：内置 v1 空库 fixture；后续每个版本补一条「vN 数据 → 最新」测试。
- 时间统一存 Unix 毫秒（`INTEGER`）；展示层再做本地化。
- 删除类操作（清空历史、删除条目）必须同步清理 FTS 行与 blob 文件。

## 5. 测试要求

| 层                            | 要求                                                                          |
|-------------------------------|-------------------------------------------------------------------------------|
| `services/ranking`、`storage` | 必须单测（内存 SQLite）                                                       |
| `platform/`                   | 不做自动化 UI 测试，按 `docs/WIN32-NOTES.md` 手动清单验证，并在任务里记录结果 |
| 前端 `lib/`                   | 必须单测；组件只测关键交互（键盘导航、空态）                                  |
| 迁移                          | 每个 schema 版本一条                                                          |

测试命名用中文描述行为，例如 `fn 三次复制同一内容只保留一条记录()`。

## 6. 代码示例

### Rust 错误处理

❌ 坏：
```rust
let item = db.get(id).unwrap(); // 生产代码禁止
```

✅ 好：
```rust
let item = db.get(id).ok_or(AppError::Storage(rusqlite::Error::QueryReturnedNoRows))?;
```

### Rust unsafe 块

❌ 坏：
```rust
unsafe { ShowWindow(hwnd, SW_SHOW); } // 缺少 SAFETY 注释
```

✅ 好：
```rust
// SAFETY: hwnd 来自 GetForegroundWindow，已在上一行判空；
// 调用期间窗口句柄不会被本模块销毁。
unsafe { ShowWindow(hwnd, SW_SHOW) };
```

### TypeScript 类型

❌ 坏：
```ts
const data: any = await invoke('clipboard_get', { id }); // 禁止 any
```

✅ 好：
```ts
const data: ClipboardDetail = await ipc.clipboardGet(id); // 走 ipc.ts，带类型
```

### 日志脱敏

❌ 坏：
```rust
log::debug!("clipboard content: {}", text); // 禁止记录内容
```

✅ 好：
```rust
log::debug!("clipboard captured: id={}, kind={}, len={}", id, kind, text.len());
```
