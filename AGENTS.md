# AGENTS.md

本文件是 QuickWin 的 AI 协作总纲。开始任何工作前先读完本文件，再读 `TASKS.md` 中当前任务的条目。

## 1. 项目一句话

QuickWin 是一个 Windows 常驻的效率工具：`Alt+Space` 启动器、`Alt+V` 剪贴板历史，技术栈为 Tauri 2 + Rust + React 19 + TypeScript。

当前阶段见 `TASKS.md` 顶部。**不要实现尚未排期或标记为「后置」的功能。**

## 2. 硬规则（违反即视为未完成）

1. **Win32 调用只允许出现在 `src-tauri/src/platform/`**。`services/` 与 `commands/` 不得直接 `use windows::...`，只能调用 platform 暴露的安全封装。
2. **前端不碰系统 API**。渲染层只能通过 `src/lib/ipc.ts` 调用 Tauri command，禁止在组件里直接 `invoke()` 散落各处。
3. **生产代码路径禁止 `unwrap()` / `expect()` / 裸 `panic!`**。统一返回 `Result<T, AppError>`。测试代码与启动期不可恢复错误（需注释说明）除外。
4. **任何数据库 schema 变更必须新增 migration**，并带迁移测试（旧库 → 新库）。禁止直接改已有 migration。
5. **剪贴板与热键必须单入口**：所有剪贴板读取/写入走 `platform/clipboard`，所有热键注册/注销走 `services/hotkey`。禁止在业务代码里临时 `RegisterHotKey` 或直接开剪贴板。
6. **不记录剪贴板内容到日志**（包括 debug 级）。日志只允许出现 id、类型、长度、耗时。
7. **不擅自新增依赖**。新增 crate / npm 包需说明理由并写进 `docs/DECISIONS.md`；能用标准库或既有依赖解决的不加。
8. **不修改验收标准**。任务做不完就拆任务或标记阻塞，不准把验收条件改松。
9. **一次只做一个任务**（`feature-slice` 技能）。禁止顺手重构无关模块。
10. **所有面向用户的文案用中文**，标识符、命令、日志字段用英文。

## 3. 环境前置

| 项       | 要求                          | 检查命令                                                              |
|----------|-------------------------------|-----------------------------------------------------------------------|
| OS       | Windows 10 1809+ / 11         | `[System.Environment]::OSVersion.VersionString`                       |
| Node     | ≥ 20（当前 24.22）            | `node -v`                                                             |
| npm      | ≥ 10（当前 11.19）            | `npm.cmd -v`                                                          |
| Rust     | stable-x86_64-pc-windows-msvc | `rustc -V`、`rustup show`                                             |
| MSVC     | VS Build Tools + C++ 桌面开发 | `vswhere -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64` |
| WebView2 | Evergreen Runtime             | 注册表或 `C:\Program Files (x86)\Microsoft\EdgeWebView`               |

**PowerShell 陷阱**：`npm` / `npx` / `pnpm` 的 `.ps1` shim 会被执行策略拦截（`UnauthorizedAccess`）。一律使用 `npm.cmd`、`npx.cmd`，或在文档中提示用户执行 `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`。不要为了让 `npm` 能跑而擅自改系统策略。

## 4. 常用命令

> 脚手架完成（T-004）后以下脚本才存在。`package.json` 中必须有同名脚本，改脚本时同步本表。

| 用途             | 命令                                                                           |
|------------------|--------------------------------------------------------------------------------|
| 安装依赖         | `npm.cmd install`                                                              |
| 开发（完整）     | `npm.cmd run tauri:dev`                                                        |
| 开发（仅前端）   | `npm.cmd run dev`                                                              |
| TS 类型检查      | `npm.cmd run typecheck`                                                        |
| 前端 lint        | `npm.cmd run lint`                                                             |
| 前端单测         | `npm.cmd run test`                                                             |
| 前端单测（监听） | `npm.cmd run test:watch`                                                       |
| Rust 格式        | `npm.cmd run rust:fmt`                                                         |
| Rust lint        | `npm.cmd run rust:clippy`                                                      |
| Rust 单测        | `npm.cmd run rust:test`                                                        |
| 质量门           | `npm.cmd run verify`                                                           |
| 打包             | `npm.cmd run tauri:build`                                                      |
| 查看数据库       | 用 DB Browser for SQLite 打开 `%APPDATA%\com.quickwin.app\quickwin.db`（只读） |

`verify` 的定义：`typecheck` + `lint` + `test` + `rust:fmt --check` + `rust:clippy` + `rust:test`。**提交前必须全绿。**

## 5. 目录结构

```
quickwin/
├── AGENTS.md                  # 本文件
├── TASKS.md                   # 任务与进度（唯一进度源）
├── docs/                      # 规格、架构、平台笔记、UI、ADR
├── .opencode/skills/          # feature-slice / tauri-win32 / release-check
├── .opencode/commands/verify.md
├── src/                       # React 渲染层
│   ├── main.tsx
│   ├── App.tsx                # 按 mode 渲染 Launcher / Clipboard / Actions
│   ├── windows/               # 三个浮层模式 + 设置窗口
│   ├── components/            # 通用组件（列表、空态、快捷键提示）
│   ├── lib/ipc.ts             # 唯一的 Tauri 调用入口（带类型）
│   ├── lib/keys.ts            # 键盘映射与导航
│   └── styles/
├── src-tauri/
│   ├── tauri.conf.json
│   ├── capabilities/default.json
│   └── src/
│       ├── main.rs / lib.rs   # 启动、插件、setup、tray
│       ├── models.rs          # 前后端共享数据结构（serde）
│       ├── error.rs           # AppError
│       ├── commands/          # Tauri command，只做参数校验与编排
│       ├── services/          # 业务：storage / search / ranking / hotkey / config / tray
│       ├── platform/          # Win32 唯一出口：input / clipboard / focus / apps / files
│       └── actions/           # 划词检测与转换（纯函数，必须单测）
└── tests/                     # 前端集成测试（vitest）
```

依赖方向：`commands → services → platform`；`actions` 为纯函数模块，可被 `services` 调用，不得反向依赖 `platform`。循环依赖一律视为设计错误。

## 6. 编码规范

### Rust

- Edition 2021，`rustfmt` 默认配置；`clippy -D warnings` 必须通过。
- 错误：`thiserror` 定义 `AppError`，对前端暴露稳定的 `code`（如 `clipboard.open_failed`）+ 中文 `message`；不把 Win32 HRESULT 直接抛给 UI。
- 异步：Tauri command 用 `async fn`，但**禁止在 `async` 中做阻塞 Win32 调用或长事务**；阻塞工作丢到 `spawn_blocking` 或专用线程。
- SQLite：单写线程 + `tokio::sync::mpsc`；读用短连接或连接池。任何 `await` 期间不得持有数据库锁或已打开的剪贴板句柄。
- `unsafe`：只允许在 `platform/` 内，且每个 `unsafe` 块上方写 `// SAFETY:` 说明前置条件与生命周期。
- 资源：HWND/HANDLE/`GlobalLock` 用 RAII 守卫封装，禁止裸 `let h = OpenClipboard(...)` 后分支提前 return。

### TypeScript / React

- `strict: true`，禁止 `any`（确需时用 `unknown` + 类型守卫）。
- 组件只负责渲染与交互，派生逻辑放 `lib/`，纯函数必须有测试。
- 所有 `invoke` 走 `lib/ipc.ts`，入参/返回值类型与 Rust `models.rs` 手工对齐；字段变更必须同步两侧并更新测试。
- 不引入 UI 组件库（Tailwind + 自建组件），保证浮层体积与启动速度。

### 命名与文案

- 任务 ID：`T-<里程碑><序号>`，如 `T-206`。
- Command：`snake_case`（`clipboard_list`）；事件：`domain://event`（`clipboard://changed`）。
- 数据库表：`snake_case` 单数（`clipboard_item`）。
- 面向用户：简体中文，不使用「您」；按钮动词开头（「粘贴」「删除」）。

## 7. 数据与迁移

- 库文件：`%APPDATA%\com.quickwin.app\quickwin.db`；附件：`blobs\<xx>\<hash>.<ext>`。
- `PRAGMA journal_mode=WAL`、`synchronous=NORMAL`、`busy_timeout=5000`、`foreign_keys=ON`。
- 版本：`PRAGMA user_version`，迁移函数数组顺序执行，单次迁移必须在事务内。
- 迁移测试：内置 v1 空库 fixture；后续每个版本补一条「vN 数据 → 最新」测试。
- 时间统一存 Unix 毫秒（`INTEGER`）；展示层再做本地化。
- 删除类操作（清空历史、删除条目）必须同步清理 FTS 行与 blob 文件。

## 8. 测试要求

| 层                            | 要求                                                                          |
|-------------------------------|-------------------------------------------------------------------------------|
| `actions/`（检测与转换）      | **必须**单测，覆盖 PRD 中的判定表，含边界与误判用例                           |
| `services/ranking`、`storage` | 必须单测（内存 SQLite）                                                       |
| `platform/`                   | 不做自动化 UI 测试，按 `docs/WIN32-NOTES.md` 手动清单验证，并在任务里记录结果 |
| 前端 `lib/`                   | 必须单测；组件只测关键交互（键盘导航、空态）                                  |
| 迁移                          | 每个 schema 版本一条                                                          |

测试命名用中文描述行为，例如 `fn 十位数字超出合理区间时不应识别为时间戳()`。

## 9. 定义完成（DoD）

一个任务只有同时满足以下条件才算完成：

- [ ] 验收标准逐条可复现
- [ ] `npm.cmd run verify` 全绿
- [ ] 新增/变更逻辑有对应测试
- [ ] 涉及 Win32 的，按手动清单在真实 Windows 上跑过，并记录环境（Win 版本、是否多屏）
- [ ] 无新增 `unwrap` / `any` / 裸 `invoke`
- [ ] 用户可见文案为中文，错误提示可操作
- [ ] `TASKS.md` 勾选并写明完成日期；有偏差的写入「实现备注」
- [ ] 没有把无关改动混进本次提交

## 10. 已知限制（不要当 bug 去修）

1. **UIPI**：无法向管理员权限窗口发送输入，也无法从其取词。只能提示用户「目标窗口权限较高」。
2. **热键占用**：`Alt+Space` 可能被 PowerToys Run / 输入法占用。处理方式是可检测、可换绑、有提示，而不是换用更底层的键盘钩子（v1 不做）。
3. **剪贴板富格式保真**：跨应用还原 HTML/RTF 时格式可能丢失；v1 只保证文本、图片、文件列表可用。
4. **首次构建慢**：`rusqlite` bundled 首次编译 30–60s，属正常现象。

## 11. 任务工作流

详见 `feature-slice` 技能。核心循环：**选任务 → 读规格 → 实现+测试 → verify → 回写 TASKS.md → 汇报**。

阻塞时标 `[!]`，写清原因与需要的决策，然后停下来问，不要猜。

## 12. 文档地图

| 想知道                     | 看                     | 何时必读                     |
|----------------------------|------------------------|------------------------------|
| 这个功能到底要什么行为     | `docs/PRD.md`          | 实现该功能前                 |
| 模块怎么分层、IPC 与表结构 | `docs/ARCHITECTURE.md` | 首次进入项目 + 改架构时      |
| Win32 怎么写、有哪些坑     | `docs/WIN32-NOTES.md`  | 碰 `platform/` 前            |
| 浮层长什么样、键盘怎么走   | `docs/UI-SPEC.md`      | 做 UI 前                     |
| 为什么这么选               | `docs/DECISIONS.md`    | 新增依赖 / 改架构前          |
| 错误码与用户提示           | `docs/ARCHITECTURE.md` §12 | 新增错误处理时           |
| 现在做到哪了               | `TASKS.md`             | 每次会话开始                 |

**上下文预算**：不要一次全读。只读当前任务相关的章节，feature-slice 技能第 2 步有具体指引。

## 13. 禁止事项

- 禁止把剪贴板内容、窗口标题、文件路径写进日志或错误上报。
- 禁止在未确认的情况下执行 `winget install`、修改系统执行策略、写注册表启动项（自启必须走 Tauri autostart 插件）。
- 禁止删除或跳过失败的测试来让 `verify` 变绿。
- 禁止在 `main` 分支直接提交未经 `verify` 的代码。
- 禁止提交 `quickwin.db`、`blobs/`、`target/`、`node_modules/`。
- 禁止在没有 migration 的情况下手工改用户库。

## 14. 提交规范

Conventional Commits，type 用英文，subject 用中文：

```
feat(clipboard): 支持图片历史与缩略图
fix(hotkey): 注册失败时提示可换绑的备选热键
docs(tasks): 完成 T-206 保留策略验收
test(actions): 补充时间戳边界用例
```

一次提交只对应一个任务或一个明确的修复。

## 15. 术语表

| 术语 | 定义 |
|------|------|
| 浮层 | 无边框置顶透明窗口，失焦自动隐藏；分 Launcher / Clipboard / Actions 三种模式 |
| 取词 | 通过模拟 `Ctrl+C` 从目标窗口获取选中文本的过程（`selection_capture`） |
| 动作 | 对取词结果的处理（转换、复制、替换），显示在动作菜单中 |
| 动作菜单 | Actions 模式下展示的「检测结果标签 + 动作列表 + 右侧预览」 |
| 保留策略 | TTL + 条数/容量上限 + 清理时机的组合（见 T-206） |
| frecency | frequency + recency，综合使用次数与最近使用时间的排序指标 |
| 质量门 | `npm.cmd run verify`，包含 typecheck + lint + test + rustfmt + clippy + rust test |
| 脱敏 | 日志/导出中只保留 id、类型、长度，不保留内容（见 §13 日志与隐私） |
| 单入口 | 剪贴板走 `platform/clipboard`，热键走 `services/hotkey`，禁止业务代码直接调用 |
| DoD | Definition of Done，定义完成清单（见 §9） |

## 16. 决策权限

| 事项 | AI 可自主决定？ |
|------|----------------|
| 任务内实现细节（算法、变量命名、文件内部结构） | ✅ |
| 新增测试用例 | ✅ |
| 修复 verify 报出的 lint / 类型错误 | ✅ |
| 新增 crate / npm 包 | ❌ 需先写 ADR 到 `docs/DECISIONS.md` + 用户确认 |
| 改目录结构（新增模块、移动文件） | ❌ 需 ADR + 用户确认 |
| 改验收标准 | ❌ 绝对禁止（见 §2 硬规则 #8） |
| 改热键默认值 | ❌ 需用户确认 |
| 改数据库 schema | ❌ 需新增 migration + 迁移测试 + 用户确认 |
| 删除或后置任务 | ❌ 只能标 `[-]` 并说明理由，不能直接删除 |
| 执行系统级操作（winget / 注册表 / 执行策略） | ❌ 需用户明确确认（见 §13） |

**原则**：实现层自由，架构层谨慎，用户可见行为不可擅自变更。

## 17. 代码示例

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
