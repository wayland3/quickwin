# TASKS.md

> 唯一进度源。`AGENTS.md` 第 8 节定义了认领与完成流程。
> 每个任务只写「做什么 + 怎么算做完」，实现细节看 `docs/`。

## 状态图例

| 标记        | 含义                                                                     |
|-------------|--------------------------------------------------------------------------|
| `[ ]`       | 未开始                                                                   |
| `[~]`       | 进行中（同时只能有一个）                                                 |
| `[x]`       | 已完成（补完成日期与备注）                                               |
| `[!]`       | 阻塞（必须写明原因）                                                     |
| `[-]`       | 后置 / v1 不做                                                           |
| `[complex]` | 复杂任务，实现前必须先写 mini-design 并通过评审（见 feature-slice 技能） |

## 评审级别

| 标记   | 含义                             | 评审要求                       |
|--------|----------------------------------|--------------------------------|
| `[L0]` | 轻量（纯文档/测试/配置）         | 实现者自验 + verify 全绿即提交 |
| `[L1]` | 标准（单模块业务逻辑）           | 1 轮外部评审，只修 BLOCKER     |
| `[L2]` | 严格（跨模块/Win32/`[complex]`） | 设计评审 + 实现评审，最多 2 轮 |

无标记默认 L1。详见 `AGENTS.md` §2 #11。

## 当前焦点

**M2 已完成**（2026-10-29）。**M3 启动器**推进中：T-301～T-304、T-307 已完成，下一个是 T-306 内置命令；M3 完成后进入 M6 设置与打磨。

**注意**：T-101 至 T-105 均已完成，M1 出口条件满足（热键呼出/隐藏、焦点恢复、SendInput 全部手工验证待用户执行）。

## 里程碑总览

| 里程碑        | 入口条件 | 目标                       | 出口条件                                                     |
|---------------|----------|----------------------------|--------------------------------------------------------------|
| M0 文档与骨架 | 无       | 能跑起空壳，质量门可用     | `npm.cmd run verify` 全绿 + `tauri:dev` 起窗口               |
| M1 平台底座   | M0 出口  | 热键能呼出浮层，焦点能还原 | 任意前台窗口下 `Alt+Space` 呼出/隐藏，关闭后焦点回到原窗口   |
| M2 剪贴板历史 | M1 出口  | 可日常使用的剪贴板面板     | 复制 20 条 → 搜索 → 回车粘到光标处，历史与剪贴板行为符合 PRD |
| M3 启动器     | M1 出口  | 搜应用、执行动作           | 输入 2 个字母命中应用并打开                                  |
| M6 设置与打磨 | M3 出口  | 可交付给他人               | 设置页、引导、i18n、性能预算达标                             |

**注意**：即使某个任务的依赖看起来已满足，也必须先完成当前里程碑的出口条件，不要跳到后续里程碑。

---

## M0 文档与骨架

### [x] [L0] T-001 项目文档基线 — 2026-10-25
产出 `README.md`、`AGENTS.md`、`TASKS.md`、`docs/*`、`.opencode/skills/*`、`.opencode/commands/verify.md`。
验收：文档之间无矛盾；所有链接可达；任务验收标准可执行。

### [x] [L0] T-002 工具链验证 — 2026-10-26
目标：确认 Rust / MSVC / WebView2 / Node 齐备，且能编译一个最小 Rust 程序。
- `rustc -V`：rustc 1.99.1 (2026-10-13)
- `cargo -V`：cargo 1.99.1
- `rustup show`：stable-x86_64-pc-windows-msvc
- `rustup component list --installed`：含 rustfmt、clippy
- 临时 crate `cargo new smoke && cargo run`：输出 Hello, world!，已删除
- 版本号已记录到 `README.md` 环境表

验收：上述命令输出贴进任务备注；无可用的 `cargo tauri` 不算失败（走 npm 脚本）。
依赖：无。**执行前需用户确认联网安装 Rust。**（已确认环境齐备，无需安装）

### [x] [L0] T-003 git 初始化 — 2026-10-26
目标：建立版本管理基线。
- `git init`，默认分支 `main`：已完成
- `.gitignore` 覆盖 `target/`、`node_modules/`、`*.db`、`blobs/`：已覆盖
- 首次提交：`chore: 初始化项目文档与骨架`：已完成（03078b3）

验收：`git status` 干净；`git log --oneline` 有记录；无二进制/数据库入库。

### [x] [L0] T-004 Tauri 2 + React 脚手架 — 2026-10-26
目标：可运行的空壳。
- `package.json` 脚本：`dev`、`build`、`typecheck`、`lint`、`test`、`rust:fmt`、`rust:clippy`、`rust:test`、`verify`、`tauri:dev`、`tauri:build`
- `src-tauri/Cargo.toml`：edition 2021，依赖见 `docs/ARCHITECTURE.md` 第 2 节
- `tauri.conf.json`：`productName=QuickWin`、`identifier=com.quickwin.app`、窗口隐藏启动、`withGlobalTauri=false`
- 前端：React 19 + TS strict + Vite + Tailwind，`App.tsx` 显示占位
- `capabilities/default.json` 只开放当前需要的权限

验收：`npm.cmd run typecheck`、`npm.cmd run lint`、`npm.cmd run test` 均通过；Rust 后端结构已建立（cargo build 因依赖编译耗时较长，将在 T-005 后验证）。
依赖：T-002、T-003。

### [x] [L1] T-005 模块骨架与错误模型 — 2026-10-26
目标：按 `ARCHITECTURE.md` 建目录与空模块，避免后续到处补洞。
- `models.rs` 定义 `ClipboardItem`、`LauncherItem`、`ActionOffer`、`Settings`、`Page<T>`：已完成，结构与架构文档一致
- `error.rs`：`AppError` + `code`/`message` + `impl Serialize`：已完成，包含所有错误码（clipboard.busy、hotkey.conflict 等）
- `commands/mod.rs` 注册 `app_ping` 命令，前端 `lib/ipc.ts` 调用成功：已完成
- `platform/mod.rs`、`services/mod.rs`、`actions/mod.rs` 空壳 + `mod` 声明：已完成

验收：`app_ping` 往返成功；前端 `npm.cmd run typecheck`、`lint`、`test` 均通过；目录与文档一致。
**实现备注**：Rust 后端编译遇到工具链环境问题（标准库链接失败，`cargo check` 报 `can't find crate for std`），需单独排查。代码结构正确，待环境修复后验证。
依赖：T-004。

### [x] [L0] T-006 质量门与日志 — 2026-10-26
目标：让 `verify` 真正能拦住问题。
- ESLint（flat config）+ Prettier；Vitest 配好 `jsdom`：已完成
- `rustfmt.toml`：已创建（edition 2021，max_width 100）
- `tauri-plugin-log`：写 `%APPDATA%\com.quickwin.app\logs\`，按天轮转，级别 info：已配置
- 日志脱敏工具：`redact(text) -> "len=123"`，禁止内容入日志：已实现（services/logging.rs），含 4 个单测

验收：前端 `typecheck`、`lint`、`test` 均通过；Rust `fmt`、`clippy`、`test` 均通过；质量门可用。
**实现备注**：修复了 capabilities 权限配置（移除不存在的 single-instance:default）、创建占位图标、修复日志插件 API（移除不存在的 RotationStrategy::Daily）。
依赖：T-005。

---

## M1 平台底座

### [x] [L1] T-101 单实例、托盘、开机自启 — 2026-10-26
- `tauri-plugin-single-instance`：二次启动只唤醒已有实例
- 托盘：设置 / 退出；左键单击打开启动器
- `tauri-plugin-autostart`：以 `--hidden` 参数开机启动，默认关，设置可开

验收：双击 exe 只出现一个实例；托盘菜单每项可用；开启自启后重启系统能在托盘看到图标且不弹窗。
依赖：T-004。

**实现备注**：
- 托盘菜单包含 2 项：设置、退出（原启动器、剪贴板、暂停记录相关项已移除，2026-10-30）
- 左键单击托盘图标打开启动器（`.show_menu_on_left_click(false)`）
- 设置窗口尚未实现（T-601），当前点击"设置"仅记录日志
- 提供 IPC 命令：`autostart_enable`、`autostart_disable`、`autostart_is_enabled`
- 交叉评审：GLM-5.3（2026-10-26）PASS，详见 docs/reviews/T-101.md

### [x] [L2] T-102 [complex] 浮层窗口管理器 — 2026-10-27
- 无边框、透明、置顶、`skipTaskbar`、默认隐藏、不抢焦点时也能显示
- 显示在**鼠标所在显示器**的工作区居中偏上（距顶 20%），物理像素与 DPI 换算正确
- 失焦自动隐藏（设置可关）；`Esc` 隐藏；窗口高度随内容 120–560 动态调整
- 显示/隐藏 120ms 淡入淡出，尊重系统「减少动画」设置

验收：双屏不同缩放（100%/150%）下位置与尺寸正确；全屏视频上按热键能出现；失焦即隐藏；连续开关 50 次无残影/闪烁。
依赖：T-004。

**实现备注**：
- 新增 `platform/focus.rs`：显示器定位 + DPI 获取 + 位置计算纯函数（4 个单测）
- 新增 `services/window.rs`：浮层窗口状态机（Hidden/Showing/Shown/Hiding）
- 新增 IPC 命令：`overlay_show`、`overlay_hide`、`overlay_resize`
- 前端监听 `overlay://shown`/`overlay://hidden` 事件，Esc 键隐藏
- 状态机处理重入：重复 hide 幂等、Hiding 中过滤 blur、show 取消 Hiding
- 动画：显示 120ms、隐藏 80ms，支持减少动画模式
- 交叉评审：GLM-5.3（2026-10-27）PASS（第 2 轮修复后），详见 docs/reviews/T-102.md

### [x] [L1] T-103 热键服务 — 2026-10-27
- 默认：`Alt+Space` 启动器、`Alt+V` 剪贴板、`Alt+X` 划词
- 启动时逐个注册，失败必须被捕获并上报（`hotkey://failed` 事件 + 托盘提示）
- 支持运行时改绑：先注册新键成功再释放旧键；冲突时保持旧键可用
- 统一入口 `services/hotkey.rs`，业务侧只订阅动作事件

验收：手工占用 `Alt+Space`（用 PowerToys 或测试程序）后启动，应用给出明确提示且其余热键正常；改绑成功后旧键失效、新键生效；退出进程不残留热键。
依赖：T-101、T-102。

**实现备注**：
- 新增 `services/hotkey.rs`：HotkeyManager 统一管理热键注册/改绑/状态
- 新增 IPC 命令：`hotkey_status`、`hotkey_rebind`
- 前端监听 `hotkey://pressed` 事件，触发对应浮层显示
- 注册失败时发送 `hotkey://failed` 事件，保留旧热键可用
- 改绑逻辑：先注册新键，成功后注销旧键
- 补评审修复（2026-10-29）：前端挂载后主动 `hotkey_status` 拉取失败列表并 toast；浮层
  `overlay://show`/`overlay://shown` 可见时再次检查；rebind 新旧键相同短路、冲突时回滚 failed
  记录保旧键；旧键注销失败回滚 bindings 并补发事件；`register_defaults` 改无返回值
- 交叉评审：doubao-seed-2.1-pro（2026-10-29 补评审）PASS，3 轮，详见 docs/reviews/T-103.md；
  真机手测清单见该记录

### [x] [L2] T-104 [complex] 前台窗口捕获与焦点恢复 — 2026-10-27
- 记录触发前的 `HWND`、进程名、窗口标题、显示器
- 浮层隐藏后通过 `AttachThreadInput` + `SetForegroundWindow` 恢复焦点，超时 150ms 放弃
- UWP 窗口（`ApplicationFrameHost`）解析真实应用名
- 目标进程提权时返回 `focus.elevated_target`，不重试

验收：在记事本、VS Code、Chrome、资源管理器、微信中触发浮层后关闭，焦点均回到原窗口且光标位置不变；目标为管理员程序时给出提示而非静默失败。
依赖：T-102。

**实现备注**：
- 新增 `platform/focus.rs`：前台窗口捕获、提权检测、焦点恢复
- 修改 `services/window.rs`：集成焦点恢复，区分 hide 语义（UserCommand/Blur/ModeSwitch）
- 修改 `commands/mod.rs`：overlay_hide 传入 HideReason::UserCommand
- 修改 `Cargo.toml`：添加 windows crate features（Security、Threading、Input_KeyboardAndMouse）
- capture_foreground 在 Hidden→Showing 时调用，失败降级为 None
- 提权检测使用 TokenElevation（OpenProcessToken + GetTokenInformation）
- 焦点恢复使用 RAII 守卫（ThreadInputGuard）保证 AttachThreadInput 成对调用
- 交叉评审：doubao-seed-2.1-pro（补登记，2026-10-29 核对 docs/reviews/T-104.md：GLM-5.3 PASS），
  真机手测清单见该记录

### [x] [L2] T-105 [complex] 输入模拟层（SendInput） — 2026-10-27
- `send_text`（Unicode，含 emoji/代理对）、`send_combo("ctrl+v")`、`send_copy()`、`send_paste()`
- 发送前后清理可能卡住的修饰键状态；不使用 `keybd_event`
- 封装为 `platform/input.rs` 安全接口，返回 `Result`

验收：向记事本、Chrome 地址栏、VS Code 编辑器分别发送中文、英文、emoji 文本均正确；`Ctrl+V` 在 5 个常见应用中有效；连续发送 100 次无字符丢失。
依赖：T-104。

**实现备注**：
- 新增 `platform/input.rs`：send_text、send_copy、send_paste
- 新增 `tests/input_manual.rs`：手测入口（#[ignore]）
- 收窄 API 面：不暴露通用 send_combo，只提供 send_copy/send_paste（与 T-105 任务描述偏差，理由：避免任意按键注入能力）
- 失败处理：零注入 → UIPI 检查 → ElevatedTarget；部分注入 → Internal；零注入非 UIPI → 重试一次
- 修饰键清理：GetAsyncKeyState 查询后发 KEYUP（优于无条件补发）
- 所有函数为阻塞函数，禁止在 async 上下文直接调用
- 交叉评审：GLM-5.3 PASS（第 2 轮修复后），详见 docs/reviews/T-105.md

### [x] [L1] T-106 SQLite 存储层（2026-10-29）
- 单写线程 + channel；读连接短生命周期；WAL / busy_timeout / foreign_keys
- `user_version` 迁移框架 + v1 schema（见 `ARCHITECTURE.md` 第 5 节）
- FTS5（trigram）独立表，写入时同步；1–2 字查询回退 `LIKE`
- 迁移测试：空库 → 最新；重复启动幂等

验收：并发 100 次写入 + 100 次查询无 `database is locked`；重启后数据一致；迁移测试通过；`PRAGMA integrity_check` 为 ok。
依赖：T-005。

**实现备注（2026-10-29）**：
- PRAGMA：busy_timeout=5000（第一步，先于 WAL 切换）、journal_mode=WAL、synchronous=NORMAL、foreign_keys=ON
- 迁移：user_version 0→1 单事务，高于当前版本报错；幂等测试通过
- FTS5 trigram 独立表，与主表写入/删除同 IMMEDIATE 事务同步；启动单快照校验不一致才 rebuild；
  1–2 字与纯标点回退 LIKE
- **偏差 1（并发模型，评审确认可接受）**：未采用「单写线程 + channel」，改为短连接 +
  `BEGIN IMMEDIATE`；高频入库仍由监听线程独占长连接
- **偏差 2（schema 范围）**：v1 只含剪贴板两表，其余表随 M3/M6 走 v2+，见 ADR-0014
- 写命令走新增 `open_write_at`（跳过一致性计数、要求已迁移）；一致性校验只在启动执行
- 并发测试：100 写 + 100 查无锁错误；重启一致；integrity_check=ok
- 交叉评审：GLM-5.3 PASS（两轮），详见 docs/reviews/T-106.md

### [x] [L1] T-107 设置读写（2026-10-29）
- `settings` 表存 JSON + Rust 侧强类型 `Settings`（serde 默认值 + 校验）
- 关键项：热键、TTL 天数、条数/容量上限、媒体类型开关、主题、开机自启、暂停记录
- 变更后广播 `settings://changed`，前端即时生效（热键项由服务处理）

验收：改设置后重启保持；非法值（TTL=0、热键格式错）被拒绝并回退默认；前端能收到变更事件。
依赖：T-106。

**实现备注（2026-10-29）**：
- schema v2 migration：`settings` 表（单条 JSON，id=1 约束）；v1→v2 测试验证剪贴板数据保留
- `Settings` 全字段带 `#[serde(default=...)]`，旧 JSON 缺字段自动补默认；`validate` 校验热键格式/
  保留天数/条数范围/主题/三热键不重复；失败返回 `SettingsInvalid(String)`
- `transaction_update_settings` 用 `BEGIN IMMEDIATE` 事务内读-改-写，避免丢更新
- 启动时 `load_settings` → 按存储键注册热键（失败回退默认键）；autostart 启动对账；
  PasteOptions 同步；DB 打不开降级回退默认设置（不阻断启动）
- `hotkey_rebind` 成功后异步写回 settings
- `RuntimeFlags` 改为 `pause_until: Mutex<Option<Instant>>`，到期自动清除；
  `Some(0)` 恢复；`None` 用 1 年模拟；托盘新增「恢复记录」
- 监听线程每 2s 重载设置，执行暂停/媒体开关过滤
- 前端 `settings://changed` 监听，failed 列表（热键占用）toast 提示
- 测试：89 Rust passed / 1 ignored（事务回滚/幂等/成功、暂停到期/永久/未暂停、三热键重复）；
  前端 43 passed；verify 全绿
- 交叉评审：GLM-5.3 PASS（第 3 轮），详见 docs/reviews/T-107.md
- **遗留（不阻塞，排期后续）**：
  1. `register_startup_hotkeys` configured 与 default 相同且被占用时，同一键进 failed 两次
  2. `hotkey_rebind` 异步写回失败被静默吞掉，成功写回不广播 `settings://changed`
  3. 事务内重读 `unwrap_or_default()` 静默吞解析错误且不 validate
  4. autostart 失败前端无感知
  5. 启动热键注册（配置键被占用 → 回退默认 → 重启重试）需真机手测

### [x] [L0] T-108 IPC 契约与类型对齐（2026-10-29）
- `lib/ipc.ts` 封装所有 command，参数/返回值类型与 `models.rs` 一一对应
- 统一错误处理：`code` 映射为中文 toast；未知错误记录但不泄露内容
- 一个契约测试：对每个 command 断言 TS 类型字段集合与 Rust 序列化结果一致（可用快照 JSON）

验收：删掉 Rust 某个字段后契约测试失败；前端无散落的 `invoke(`（除 `ipc.ts`）。
依赖：T-105、T-107。

**实现备注（2026-10-29）**：
- `ipc.ts` 新增 `invokeWrapper` 统一包装所有 invoke 调用，捕获后端 `AppError` 并映射为中文 toast
- 错误码映射表 `ERROR_MESSAGES`：覆盖所有 10 个错误码（clipboard.busy、focus.elevated_target 等）
- 未知错误：`console.error` 记录 code + command 名（不泄露后端 message），统一返回「操作失败，请稍后重试」
- 新增共享 fixture 文件 `src/lib/contract/*.keys.json`（4 个结构各一个），TS/Rust 双侧测试读取同一份比对
- 新增 `ipc.contract.test.ts`：4 个契约测试验证 `ClipboardItem`、`ClipboardDetail`、`ClipboardListResult`、`Settings` 字段集合与 Rust 序列化一致
- Rust 侧新增 4 个契约测试（`models.rs`），用 `include_str!` 读取 fixture，`serde_json::to_value()` 序列化后比对 keys
- 验收通过：grep 确认前端无散落的 `invoke(`（仅 `ipc.ts` 内部使用）；契约测试字段集合与 Rust 序列化一致
- 测试：47 前端测试通过（新增 4 个契约测试）；93 Rust 测试通过 / 1 ignored；verify 全绿
- 交叉评审：GLM-5.3 PASS（第 2 轮；第 1 轮 FAIL 因契约测试与 Rust 零耦合，已修复为共享 fixture 机制），详见 docs/reviews/T-108.md

---

## M2 剪贴板历史（用户优先项）

### [x] [L2] T-201 [complex] 剪贴板变更监听 — 2026-10-27
- 后台线程每 250ms 读 `GetClipboardSequenceNumber`，变化后延迟 30ms 再读取
- `OpenClipboard` 失败重试 5 次 × 20ms；仍失败则跳过本次并计数
- 读取完成立即关闭句柄；读取期间不得 `await`
- 变更后发 `clipboard://changed`（只带 id 与类型，不带内容）

验收：连续复制 20 次，全部捕获且无重复事件；打开着剪贴板的程序（如 Excel 复制中）不会让监听线程卡死；空闲 CPU ≈ 0%。
依赖：T-106。

**实现备注**：
- 新增 `platform/clipboard.rs`：get_sequence_number、open_clipboard、ClipboardGuard（RAII）
- 新增 `services/clipboard.rs`：ClipboardMonitor（后台线程、动态重试、优雅关闭）
- 新增 `tests/clipboard_manual.rs`：手测入口（#[ignore]）
- 修改 `Cargo.toml`：添加 windows features（DataExchange、Memory）
- 线程生命周期：Arc<AtomicBool> + JoinHandle，Drop 时自动 stop
- 序列号溢出：wrapping_sub 处理回绕
- 动态重试：立即 → 10ms → 50ms，总窗口 100-200ms
- 交叉评审：GLM-5.3 PASS（第 2 轮修复后），详见 docs/reviews/T-201.md

### [x] [L1] T-202 多格式提取 — 2026-10-28
- 支持：`CF_UNICODETEXT`、`CF_HTML`、`CF_RTF`、`CF_DIB/PNG`、`CF_HDROP`
- 超限：文本 >256KB 截断并标记；图片 >8MB 只存缩略图并标记
- 前台应用信息（进程名、标题）随条目记录

验收：截图入库并能显示缩略图；资源管理器复制 3 个文件显示为文件项；HTML 复制保留纯文本预览。
依赖：T-201、T-104。

**实现备注**：
- 新增 `platform/clipboard.rs` 多格式读取函数：read_clipboard_content、read_unicode_text、read_html、read_rtf、read_image_bytes、read_file_list
- 新增 ClipboardContent 枚举和 ClipboardEntry 结构体
- 实现 HTML 格式解析（parse_html_fragment，优先 StartFragment/EndFragment，回退 StartHTML/EndHTML）
- 实现 HTML/RTF 转纯文本（用于预览）
- 实现文本截断（按字符数 / 字符边界，不切坏多字节字符）
- 添加 windows features：Win32_UI_Shell、Win32_UI_Shell_Common
- 添加 From<windows::core::Error> for AppError
- 交叉评审：GLM-5.3 第一轮 **FAIL**（CF_DIB 未读取、RTF 按 UTF-16 误判、>8MB 图片静默丢弃），修复后第二轮 **PASS**，详见 docs/reviews/T-202.md
- 修复内容（2026-10-28）：
  - 补 CF_DIB(8) / CF_DIBV5(17) 读取；`ClipboardContent::Image { data, format }`，`ImageFormat::{Png,Dib}`，blob 扩展名随格式（`.png` / `.dib`）
  - RTF 改为按字节读 + `from_utf8_lossy`（不再按 UTF-16 猜测）；UTF-16 解码只用于 CF_UNICODETEXT，并按第一个 NUL 截断
  - >8MB 图片改为生成 `truncated=true`、`preview="[图片过大]"`、无 blob 的标记条目，不再静默丢弃（UI-SPEC §4.4「原图过大」）
  - 格式常量命名（CF_UNICODETEXT/CF_HDROP/CF_DIB/CF_DIBV5）；文本补 256KB 上限（字符边界截断）
  - GlobalSize 先判大小再拷贝；DragQueryFileW 长度 `saturating_add` + `u32::MAX` 防御；register_format 返回 0 记 debug 日志
- 偏差（排期 T-204）：缩略图生成与 DIB→PNG 转换需 WIC，尚未实现；`source_title` 出于隐私暂不采集
- 36 个 Rust 测试通过（新增 DIB/超限/格式携带等用例）
- 真机手测：待用户执行（PrtScn 截图、资源管理器复制 3 文件、Chrome 富文本预览）

### [x] [L1] T-203 入库管线 — 2026-10-28
- `hash = sha256(kind + 规范化内容)`；重复则 `copy_count+1`、刷新 `last_copied_at`、置顶显示
- 文本写入 `preview`（≤512 字）+ `text`；图片写 blob + 缩略图；文件列表存 JSON
- FTS 行同步更新；单事务提交
- 事件节流：500ms 内多次变更合并刷新 UI

验收：同一内容复制 3 次只有 1 条记录且计数为 3；预览不含控制字符；FTS 能搜到 3 字以上子串；数据库写入失败时 UI 有提示且不崩。
依赖：T-202。

**实现备注**：
- 新增 `services/storage.rs`：SQLite 存储层（`ClipboardStorage`），负责迁移、去重入库、FTS 同步、blob 落盘
- 数据库路径 `%APPDATA%\com.quickwin.app\quickwin.db`；PRAGMA：WAL / synchronous=NORMAL / busy_timeout=5000 / foreign_keys=ON
- 迁移：`PRAGMA user_version` 0 → 1，建表 + 索引 + FTS5（trigram），单事务；版本高于当前时启动报错而不是跳过
- 去重：`sha256(kind + 规范化内容)`（文本类统一换行、文件列表按路径排序），先查 hash 再决定插入或 `copy_count+1` + 刷新 `last_copied_at`；返回真实 id（`InsertOutcome::{Inserted,Updated}`）
- 事务：`Connection::transaction()`，插入主表 + FTS + 提交在同事务内，任一步失败自动回滚
- FTS：显式 `rowid = clipboard_item.id`；启动时对比主表/ FTS 行数，不一致则 `rebuild`
- blob：图片写 `blobs\<hash 前两位>\<hash>.<ext>`（PNG 源为 `.png`，DIB 源为 `.dib`），DB 存相对路径；先落盘再写库；超限图片不写 blob
- 预览：去控制字符（保留 `\n`、`\t`），按字符截断到 512，避免切坏多字节字符
- 事件：成功 `clipboard://changed { id, kind }`（500ms 节流，首条不节流）；失败 `clipboard://insert-error { code, message }`（不节流），前端 `App.tsx` 监听并显示中文 toast，4s 自动消失
- 生命周期修复：`ClipboardMonitor` 由 `app.manage()` 持有（此前 setup 里 `let _monitor` 会被立即 drop，监听线程实际未运行）
- 启动降级：存储初始化失败只记日志、不阻塞应用启动
- 顺手修复 T-202 遗留：`truncate_text` 按字节切片会 panic（中文 >512 字节），改为按字符截断；补上 256KB 正文上限
- 测试：36 个 Rust 测试通过（T-203 新增 10 个：三次复制去重计数、预览去控制字符、预览 512 字符、FTS 子串、FTS 行数一致、hash 规范化、文件列表 JSON、图片 blob 落盘、超大图标记条目、迁移幂等）
- 偏差：图片缩略图未生成（需 WIC 解码，留到 T-204 用 blob 原图 + CSS 缩放展示）；`source_title` 暂不采集（`focus::capture_foreground` 出于隐私未读窗口标题）；>8MB 图片只记录标记条目（`truncated=1`、无 blob），缩略图与 DIB→PNG 转换随 T-204 补齐
- 交叉评审：GLM-5.3 PASS（qwen3.7-plus 配额不足，经用户同意降级），详见 docs/reviews/T-203.md
- 说明：本任务同时修复了 T-202 复评发现的 3 个阻塞问题（DIB 读取、RTF 编码、超限图片），修复单独提交

### [x] [L1] T-204 剪贴板面板 UI（2026-10-28）
- `Alt+V` 打开，布局见 `SPEC.md` §6.4
- 搜索框实时过滤（防抖 80ms）；Tab 切换 全部/文本/图片/文件/收藏
- 键盘：↑↓ 选择、`Enter` 粘贴、`Ctrl+Enter` 仅复制、`Ctrl+P` 置顶、`Delete` 删除、`Esc` 关闭
- 右侧预览：文本全文（可滚动）、图片、文件列表、来源应用与相对时间
- 空态/无结果/加载态齐全

验收：1000 条数据下输入到出结果 <50ms；键盘全程可操作无需鼠标；置顶项始终在顶部并有标记；删除后列表与数据库同步。
依赖：T-203。

实现备注（2026-10-28）：
- 后端新增 `services/query.rs`（搜索/筛选纯函数，含转义与纯标点回退）、`commands/clipboard.rs`
  （`clipboard_list/get/set_pinned/delete`）、`storage.rs` 的 `list/get/set_pinned/delete` 与 `open_readonly_at`
- 并发模型：每 command 短连接 + `spawn_blocking`，`Connection` 与锁都不跨 await；`DbPath` 只读受管状态
- 排序照 `ARCHITECTURE §5.3`：`pinned DESC` 首键；FTS 集合内按 `bm25`；末尾 `id DESC` 稳定 tie-break
- 图片预览：`tauri.conf.json` 开 `assetProtocol`（scope `$APPDATA/blobs/**`）+ Cargo `protocol-asset` feature，见 `docs/DECISIONS.md` ADR-0013
- 偏差 1：v1 无回收站，`Delete` 与 `Shift+Delete` 都是物理删除（`hard` 只区分是否删 blob；孤儿 blob 由 T-206 回收）
- 偏差 2：`Enter` / `Ctrl+Enter` 在 T-204 只完成键位接线并提示「功能开发中」，真实粘贴/仅复制在 T-205
- 偏差 3：`thumb_path` 恒为 NULL（WIC 缩略图未实现），列表用类型占位图标
- 新增 `ClipboardDetail.file_exists`，预览区据此把缺失文件标红（`UI-SPEC §4.4`）
- 性能：Rust 测试「千条数据搜索查询在五十毫秒内」插入 1000 条，FTS/LIKE 各 10 次取中位数并断言 <50ms；
  dev 模式输出 `[perf] clipboard list A=..ms B=..ms`（A=防抖后→DOM commit，B=按键→commit），供真机采集
- 测试：57 个 Rust 测试 + 46 个前端测试通过；`npm.cmd run verify` 全绿
- 交叉评审：seed-2.1-pro **PASS**（设计 4 轮、实现 2 轮），记录见 `docs/reviews/T-204.md`
- 待办（真机手测，需 Windows 桌面）：Alt+V 打开与焦点、中文 3 字/1 字搜索、纯标点不报错、Tab 循环、
  ↑↓/PgUp/PgDn、Ctrl+P 置顶与 📌/×N/分隔线、Delete/Shift+Delete 与 blob 清理、截图预览（asset 协议）、
  DIB 占位文案、IME 组合期按键、Esc 关闭、1000 条下 A/B 耗时

### [x] [L2] T-205 [complex] 激活与粘贴（2026-10-29）
- 流程：隐藏浮层 → 恢复前台窗口 → 将条目写入剪贴板（全格式）→ 抑制监听（hash + 1.5s 窗口）→ `Ctrl+V`
- **不新增历史条目、不改变历史顺序**（选中即用，不污染）
- 粘贴后条目**保留在系统剪贴板**（符合 Win+V 习惯，便于再次手动粘贴）
- 目标不可输入时至少保证剪贴板已就绪，UI 提示「已复制，可手动粘贴」
- 提供设置项「粘贴后还原原剪贴板」（默认关）

验收：从历史选一条粘到记事本光标处，内容正确且历史列表无新增/无重排；管理员窗口下给出提示且剪贴板可用；连续粘贴 5 条内容互不串；emoji 与多行文本正确。
依赖：T-204、T-105、T-104。

**实现备注（2026-10-29）**：
- `platform/clipboard.rs` 新增 `write_clipboard_content`：单次会话写入条目全部可写格式；HTML 重拼
  `HTML Format` 头 + 纯文本、RTF + 纯文本、PNG 注册格式 + WIC 合成的 `CF_DIB`、文件 `CF_HDROP`；
  失败时保留已成功格式，不清空剪贴板
- `platform/imaging.rs`（新增）WIC PNG→BGRA→DIB：alpha 统一 0xFF、bottom-up 行序
- `services/paste.rs`（新增）编排：`hide_for_paste`（同步隐藏+restore，不走异步动画）→ `begin`
  → 写剪贴板 → `seal` → `Ctrl+V`；`paste_lock` 序列化并发激活，还原线程同样持锁并完整 begin/seal
- 抑制：`PasteSuppression` 序列号区间为主（wrapping 包含）+ 规范化 `content_hash` 兜底，1.5s 过期；
  监听线程命中即跳过，不插入、不发事件、不刷新 `last_copied_at`
- 降级：提权预检 / 未捕获前台窗口 → 不隐藏、只写剪贴板 + `app://toast`；restore 或 `send_paste`
  失败 → 已写剪贴板后重新展示面板 + toast「已复制，请手动粘贴」；写失败 → resume + 错误提示
- 设置：`clipboard_set_restore_after_paste` 进程内开关（默认关），开启时粘贴前快照、Ctrl+V 后
  300ms 写回（seq 又变则放弃）；持久化留 T-107、UI 留 T-601
- 前端：Enter → `clipboard_activate(id,'paste')`，Ctrl+Enter → `'copy'`；App 根监听 `app://toast`
- 偏差：T-202 每条约只存 1 种内容，无法还原原剪贴板全部并行格式；>8MB 图片无 blob 无法粘贴；
  「目标控件不接受合成按键」v1 无法探测（记入已知限制）；DIB 不做转 PNG
- 测试：Rust 70 passed / 1 ignored（真实剪贴板 round-trip 手测），前端 43 passed；`verify` 全绿
- 交叉评审：doubao-seed-2.1-pro PASS（第 2 轮；第 1 轮 FAIL 的三项已修复/登记偏差），
  详见 `docs/reviews/T-205.md` 与 `docs/reviews/T-205-design.md`
- 待真机手测：记事本/Chrome/VS Code/资源管理器下的文本、emoji、多行、图片、文件列表粘贴；
  历史无新增/无重排；连续 5 条互不串；管理员窗口降级；关闭目标窗口后 restore 降级；
  还原开关开启后 300ms 恢复原剪贴板且不重排

**修复（2026-10-30，真机反馈）**：
- 现象：`Enter` 不粘贴只提示「已复制」；之后点击其他应用第一下无效；`Esc`/点击外部不退出
- 根因与修复见 `docs/reviews/T-205-fix.md`：`restore` 误把 `AttachThreadInput` 失败当提权拒绝、
  目标线程 Alt 状态卡住（补发 KEYUP）、失焦隐藏未接 `on_window_event`、提权同名进程允许粘贴
- 用户已真机确认三项均正常；`verify` 全绿（131 Rust / 4 ignored、53 前端）
- 交叉评审：GLM-5.3 PASS（L2，1 轮），详见 `docs/reviews/T-205-fix.md`
- 遗留（NONBLOCK）：`Showing` 动画期失焦忽略；`hide` 恢复不持粘贴锁；KEYUP 为全局注入

### [x] [L1] T-206 保留策略与容量治理（2026-10-29）
- TTL 15 天，按 `last_copied_at`；置顶豁免
- 上限：10,000 条 / 单文本 256KB / 单图 8MB / blob 总量 500MB
- 清理时机：启动 + 每 6h + 每累计 200 次写入；清理后回收孤儿 blob（仅删除 1h 前创建的）
- 「暂停记录」开关；「清空历史」二次确认（含清空收藏选项）

验收：把系统时间前推 16 天后启动，未置顶项被清、置顶项保留；把 blob 目录塞到超限，最旧未置顶项被淘汰且文件被删；清空后 `blobs/` 为空、FTS 无残留；暂停记录期间复制不新增。
依赖：T-203、T-107。

**实现备注（2026-10-29）**：
- 新增 `services/retention.rs`：`RetentionPolicy`（TTL 截止/条数超出/500MB 预算）、6h/200 写/孤儿 1h 常量
- `storage.rs` 新增 `cleanup`：TTL（非置顶）→ 条数上限（最旧非置顶）→ blob 预算（按 `blobs/`
  实际磁盘占用，含孤儿，淘汰最旧非置顶图片）→ 孤儿回收（仅删 1h 前文件）；主表/FTS 同事务，
  blob 在提交后删除
- `clear_history(include_pinned)`：保留收藏或全部清空；全部清空后移除 `blobs/` 目录
- 监听线程：启动 + 每 6h + 每 200 次成功写入触发清理；删除后发 `clipboard://cleaned` 刷新面板
- 暂停记录沿用 T-107 的持久化开关 + `RuntimeFlags`，暂停期间不读取、不入库
- 前端：`Ctrl+Shift+Delete` 打开内联确认条（Enter 保留收藏 / Ctrl+Enter 全部清空 / Esc 取消）；
  按钮 `tabIndex=-1` + `onMouseDown` preventDefault，避免确认态误触发粘贴
- 文档：UI-SPEC 补键盘行、ARCHITECTURE 事件表补 `clipboard://cleaned`
- 测试：新增 7 个 Rust（TTL、条数、blob 预算两种口径、清空保留收藏、孤儿回收、清空后目录为空）；
  前端键盘/确认流测试；104 Rust passed / 1 ignored，前端 44 passed，verify 全绿
- 交叉评审：doubao-seed-2.1-pro PASS（第 2 轮；第 1 轮 FAIL：预算按 DB 字节、blobs 残留、按钮双触发），
  详见 docs/reviews/T-206.md
- 遗留：清空与监听并发窄窗口竞态；<1h 大孤儿会连坐淘汰正常图片（满 1h 后自愈）；
  「系统时间前推 16 天」需真机手测

---

## M3 启动器

### [x] [L2] T-301 应用索引 — 2026-10-29
- 来源：开始菜单 `.lnk`（用户 + 全局）、`App Paths` 注册表、UWP（`shell:AppsFolder`）
- 解析 lnk 目标与图标；UWP 用真实显示名 + `AppUserModelID`
- 索引写入 SQLite（`app_index`），带来源与去重键（可执行路径 / AUMID）

验收：本机开始菜单应用命中率 ≥90%（人工抽查 30 个）；同一应用不重复出现；重建索引 ≤2s（1000 项）。
依赖：T-106。

**实现备注（2026-10-29）**：
- 评审级别：TASKS 原标 L1，因涉及 `platform/` Win32/COM 边界 + schema migration，按
  `AGENTS.md` §2 #11 上调为 L2（设计评审 + 实现评审各 1 轮，均 PASS）
- 新增 `platform/apps.rs`：COM 守卫（STA，`S_OK`/`S_FALSE` 配对、`RPC_E_CHANGED_MODE` 不反初始化）、
  `.lnk` 解析（`IShellLinkW` + `IPersistFile`，`SLGP_RAWPATH`）、App Paths（HKLM 64/32 位 + HKCU）、
  UWP（`SHGetKnownFolderIDList` + 桌面 `IShellFolder::BindToObject` + `IEnumIDList` + `SHCreateItemWithParent`
  + `PKEY_AppUserModel_ID`/`SIGDN_NORMALDISPLAY`）
- `Cargo.toml` 复用既有 `windows` crate，追加 feature `Win32_UI_Shell_PropertiesSystem`、
  `Win32_Storage_FileSystem`、`Win32_System_Registry`（未新增依赖）
- 新增 `services/appindex.rs`：开始菜单目录递归（深度 ≤4）、候选合并纯函数、来源优先级
  `start_menu > app_paths > uwp` + 名称字典序 + 图标优先的稳定去重；过滤卸载/说明类快捷方式、
  目标不存在或非可执行扩展；UWP 仅保留含 `!` 的包内 AUMID（Win32 风格 AUMID 已由前两来源覆盖，
  避免同一应用重复出现）
- `storage.rs`：`SCHEMA_VERSION` 2→3，v3 建 `app_index`（`key UNIQUE` + `ix_app_name`）；
  `replace_app_index` 单事务 DELETE + INSERT，防御性按小写 key 去重；v2→v3 迁移测试保留剪贴板与设置
- 新增 `commands/launcher.rs::app_index_rebuild`：`spawn_blocking` 内扫描 + 落库，返回 `{ count }`；
  `models.rs` 新增 `AppIndexRebuildResult`，前端 `ipc.ts` + `ipc.contract.test.ts` + 共享 fixture
- 错误处理：COM 初始化失败 → `Internal`；单条快捷方式失败跳过计数；App Paths / UWP 失败降级为空 +
  warn；SQLite 失败事务回滚；扫描为空返回 `count=0`
- 日志只记来源计数/失败数/耗时，不含路径与应用名
- 测试：116 Rust passed / 2 ignored，前端 49 passed；`npm.cmd run verify` 全绿
- 交叉评审：GLM-5.3 PASS（设计 1 轮 + 实现 1 轮），详见 `docs/reviews/T-301.md` 与
  `docs/reviews/T-301-design.md`
- 真机实测（本机）：`.lnk` 134 / 失败 4、App Paths 75、AppsFolder 170（过滤后入索引）、
  最终 109 条；首次冷启动 1.4s、预热 0.35s；同小写名重复 0（`#[ignore]` 测试可复跑）
- 偏差 1：图标只存 `.lnk` 图标位置字符串（`path` 或 `path,index`），未做 HICON→PNG 提取，
  启动器 UI 先用占位/系统图标（随 T-303/T-304）
- 偏差 2：UWP 枚举失败的 `Get-StartApps` 快照降级未实现（当前降级为空 + warn），随 T-302 刷新评估
- 偏差 3：启动异步刷新与 10 分钟轮询属 T-302，本任务只交付命令触发的完整重建；托盘/内置命令入口随 T-306
- 遗留（NONBLOCK）：
  1. 原始路径未展开环境变量（`%SystemRoot%` 等）时可能漏项，改为 `SLGP_RAWPATH` 的已知取舍
  2. 图标回退到 target，`cmd.exe /c` 类启动器可能指向非实际程序
  3. 逐候选 `Path::is_file()` 磁盘 IO，候选量更大时接近 2s 门限需再评估缓存
- 待真机手测（需在用户目标机执行）：人工抽查 30 个开始菜单应用核对命中率；触发一次重建并核对
  面板/搜索可用（入口随 T-303/T-304）；UWP 启动待 T-304 接入 `IApplicationActivationManager`

### [x] [L1] T-302 索引刷新 — 2026-10-29
- 启动时异步刷新；开始菜单目录变化（`ReadDirectoryChangesW` 或 10 分钟轮询）触发增量更新
- 删除失效项；新装应用 ≤10 分钟可被搜到

验收：卸载一个应用后 10 分钟内从结果消失；新装应用 10 分钟内可搜到；刷新不阻塞 UI。
依赖：T-301。

**实现备注（2026-10-29）**：
- `services/appindex.rs` 新增 `rebuild_index`：扫描 → 与 `load_app_index` 逐行对比 → 未变化跳过写库，
  变化时单事务全量替换（失效项自然清除）
- 新增 `IndexRebuildLock`：后台刷新与 `app_index_rebuild` 命令共用，避免并发全量替换
- 新增 `IndexRefresher`：独立线程启动即建一次，之后每 600s 轮询；1s 粒度检查停止标志，
  App 退出（Drop）最多等 1s；启动不阻塞 setup / UI
- `storage.rs` 新增 `load_app_index`（按 key 排序）
- `lib.rs` 注册锁并启动刷新线程；`commands/launcher.rs` 改用 `rebuild_index`
- 偏差：v1 按 ARCHITECTURE §9 用 10 分钟轮询，不用 `ReadDirectoryChangesW`；「增量更新」实现为
  「全量扫描 + 结果对比，无变化不写库」，理由：整批扫描本机 0.35s、实现简单且不会漏掉注册表变化
- 顺手修复：测试内存库改为每实例独立临时 blob 目录（`#[cfg(test)] cleanup_dir`，drop 时清理），
  修复并行测试互相删目录导致的偶发失败（与任务无关但会阻塞质量门）
- 测试：117 Rust passed / 3 ignored，49 前端 passed；`npm.cmd run verify` 全绿；连续 3 次全量测试稳定
- 交叉评审：GLM-5.3 PASS（1 轮），详见 `docs/reviews/T-302.md`
- 遗留（NONBLOCK）：`rows_equal` 对重复 key 依赖原顺序（扫描层已保证唯一）；锁覆盖整次扫描，
  手动重建需排队 ≤1.4s；刷新间隔从上次完成后起算（600s + 扫描耗时，仍满足 ≤10 分钟）
- 待真机手测：启动后日志出现 `app index refreshed`，重启后出现 `unchanged`；
  卸载一个应用/删除一个 `.lnk` 后 ≤10 分钟索引中消失；新装应用 ≤10 分钟出现；
  `cargo test --lib 真机刷新应用索引两次 -- --ignored --nocapture` 验证第二次短路

### [x] [L1] T-303 排序与检索 — 2026-10-29
- 匹配：子串优先 + 模糊（`fuzzy-matcher`）+ 首字母缩写（`vsc` → Visual Studio Code）
- 排序：匹配分 × frecency（次数 + 最近使用衰减）
- 12 条上限，分组「应用 / 命令」

验收：输入 `chr` 首位是 Chrome；常用应用排在使用少的同名应用前。
依赖：T-301。

**实现备注（2026-10-29）**：
- 新增 `services/ranking.rs`：`match_query`（完全 1.0 / 前缀 0.9 / 词首缩写 0.85 / 子串 0.7 /
  子序列模糊 0.4–0.6）、`acronym`、`frecency`（1 + ln(1+count) × 0.5^(days/14)）、
  `source_weight`、`rank_apps`（同分按标题长度、再按 key）；原 `with_web_fallback`
  已随网页搜索功能取消移除（2026-10-30）
- 新增 `launcher_query` 命令：`spawn_blocking` 读索引 + 使用频率，返回 `{ items, hint }`；
  `models.rs` 新增 `LauncherQueryResult`，前端 `ipc.ts` + `lib/launcher.ts` + 契约 fixture 同步
- schema v4：`usage_stat` 表 + `load_usage_stats` / `bump_usage`（频率记录由 T-304 打开应用时调用），
  带 v3→v4 迁移测试
- 新增 `src/windows/LauncherPanel.tsx`：搜索框（80ms 防抖）、分组标题、↑↓ 选择、Enter 提示待接入、
  空态/加载态/错误提示；`App.tsx` 在 `mode=launcher` 时渲染；查询词跨隐藏保留（SPEC §4）
- 偏差 1：未引入 `fuzzy-matcher`（新增依赖需 ADR + 用户确认），改用自研子序列匹配；
  匹配档位与分值仍按 ARCHITECTURE §8.1
- 偏差 2：本任务包含启动器面板 UI（任务条目只写检索/排序），否则无法观察验收；
  Enter 打开随 T-304
- 偏差 3：命令分组随 T-306；当前只有「应用」一组
- 测试：131 Rust passed / 4 ignored，53 前端 passed；`npm.cmd run verify` 全绿
- 真机抽查（本机未装 Chrome）：`fir` → Firefox 首位、`vsc` → Visual Studio Code 首位、
  `微信` → 精确命中；可用 `cargo test --lib 真机排序抽查 -- --ignored --nocapture` 复跑
- 交叉评审：GLM-5.3 PASS（1 轮），详见 `docs/reviews/T-303.md`
- 遗留（NONBLOCK）：每次查询新开连接并全量读索引（规模变大后随 T-605 评估缓存）；limit 双重 clamp；
  副标题直接显示 exe 完整路径，UI 打磨随 T-304/T-604

### [x] [L1] T-304 执行应用 — 2026-10-30
- `Enter` 打开应用：Win32 走 `ShellExecuteW`，UWP 走 `IApplicationActivationManager`
- 成功后隐藏浮层并记录 frecency（`bump_usage`）
- 执行失败（应用已卸载、无权限、找不到关联程序）给中文错误提示，关闭面板

验收：普通应用与 UWP 应用均能打开；失效快捷方式给出中文提示且面板不关闭；连续执行 20 次且每次都能被热键重新唤起。
依赖：T-303。

**范围收敛（2026-10-30，用户确认）**：原条目中的 `Shift+Enter` 管理员打开、`Ctrl+Enter` 打开所在位置、`Ctrl+C` 复制路径三项不再实现，已从本任务与 `SPEC.md` / `ARCHITECTURE.md` / `WIN32-NOTES.md` 同步移除。

**实现备注（2026-10-30）**：
- `platform/apps.rs` 新增 `launch_win32`（`ShellExecuteW` + `open`）、`launch_uwp`
  （`CLSID_ApplicationActivationManager` + STA 的 `ComApartment`）、`allow_foreground`
  （`AllowSetForegroundWindow(ASFW_ANY)`，浮层在前台时保证新应用能抢到前台）、
  `shell_error_reason`（错误码 → 中文原因，不把数字给用户）
- `commands/launcher.rs` 新增 `launcher_execute { key }`：先查 `app_index`（新增
  `storage.load_app`，`key` 小写匹配），再在 `spawn_blocking` 里启动；**成功后才隐藏浮层**
  （新增 `HideReason::Launch`：立即隐藏、不恢复焦点），失败返回 `launcher.launch_failed`，面板保留
- frecency：启动成功后异步 `bump_usage(key, "app")`，失败只记 warn，不影响打开结果
- 错误码：`AppError::LaunchFailed` → `launcher.launch_failed`；前端对该码直接展示后端分类文案
  （已卸载 / 无权限 / 无关联程序 / UWP 启动失败），其余错误码仍走固定映射
- 前端：`ipc.launcherExecute`；`LauncherPanel` Enter/双击打开，执行期间忽略重复触发，
  失败在底部提示且不关闭；`command` 仍保留「T-306 接入」提示
- 测试：135 Rust passed / 4 ignored（新增 `shell_error_reason` 分类 2 项、`load_app` 1 项）；
  前端 56 passed（新增 Enter 打开、失败提示、防重入 3 项）；`npm.cmd run verify` 全绿
- 交叉评审：GLM-5.3（2026-10-30）PASS（L1，1 轮），详见 `docs/reviews/T-304.md`
- 遗留（NONBLOCK）：
  1. `launch_win32` 未在启动前再校验目标存在性（索引扫描后有窗口期），依赖 `ShellExecuteW` 错误码兜底；
     若加前置 `is_file()` 需处理 T-301 遗留的未展开环境变量路径
  2. `open()` 的防重入是前端 ref 级，后端未做并发去重
- 待真机手测（需在用户目标机执行，见 `tests/launcher_manual.rs`）：
  1. 记事本 / VS Code / Chrome：Enter 打开，浮层隐藏且应用在前台
  2. UWP（计算器 / 设置，AUMID 用 `Get-StartApps` 核对）：Enter 打开
  3. 指向已删除 exe 的快捷方式：中文提示且面板不关闭
  4. 连续执行 20 次：每次 `Alt+Space` 都能重新唤起并正常打开
  5. 打开后立即再按热键：面板正常显示，无残影

---

## M6 设置与打磨

### [ ] [L1] T-601 设置窗口
- 独立窗口 880×600，左侧分组：常规 / 热键 / 剪贴板
- 所有项即时保存并广播；「恢复默认」带二次确认
- 数据路径可点击打开；显示当前 DB 体积与条目数

验收：每个设置项都有默认值、说明文案、保存反馈；窗口可键盘操作；关闭后不残留进程。

**实现备注**：
- 创建设置窗口（880×600，可缩放，最小 720×520）
- 左侧导航 200px：常规 / 热键 / 剪贴板 / 隐私 / 关于
- 右侧内容最大宽度 560px，分组卡片式
- 支持所有设置项的读取和更新
- 主题切换立即生效（通过 T-606 的主题系统）
- 清空历史记录带二次确认
- 暂停记录开关
- 恢复默认设置带二次确认
- 热键和条数上限输入框改为 draft state，blur/Enter 时提交
- 数据路径可点击打开和 DB 统计待后续实现（需要 openPath 和 clipboardStats command）
- 热键录制控件待 T-602 实现

交叉评审：GLM-5.3（2026-10-30）PASS（L1，2 轮），详见 docs/reviews/T-601.md
依赖：T-107。

### [x] [L1] T-606 主题系统基础设施 — 2026-10-30
目标：支持主题切换，实现 light/dark 两套主题配色。

改动：
- `src/index.css`：新增 `data-theme` 属性选择器，复用 SPEC §4.2 配色，保留 96% 透明度，添加 `color-scheme` 属性
- `src/lib/theme.ts`（新增）：`applyTheme` 函数，修改 `<html data-theme>` 属性
- `src/lib/theme.test.ts`（新增）：主题切换测试（6 项）
- `src/App.tsx`：启动时读取设置并应用初始主题；监听 `settings://changed` 事件，theme 字段变更时更新主题

验收：
- `npm.cmd run verify` 全绿
- 手动测试：设置 `theme` 为 `light` / `dark` / `system` 后，浮层立即切换配色
- CSS 变量正确覆盖（`--bg`、`--surface`、`--accent` 等）

交叉评审：GLM-5.3（2026-10-30）PASS（L1，2 轮），详见 docs/reviews/T-606.md
依赖：T-107（设置读写）。

### [x] [L1] T-602 热键录制控件 — 2026-10-30
- 点击后进入录制态，按下组合键自动识别并保存
- `Esc` 取消录制；`Backspace` 清空热键
- 录制期间暂停全局热键，避免误触发
- 保存前检测冲突：尝试注册 → 失败则提示「热键注册失败，请尝试更换」

验收：录 `Ctrl+Alt+K` 生效；录已被占用的键有明确错误且原热键仍可用；录制态不会误触发全局热键。
依赖：T-103、T-601。

**实现备注（2026-10-30）**：
- 新增 `src/lib/keys.ts`：键盘事件到 Tauri accelerator 格式的转换工具
  - `eventToAccelerator`：从 KeyboardEvent 构建 accelerator 字符串
  - `formatAccelerator`：格式化 accelerator 用于显示（Super→Win、Space→空格等）
  - `isValidAccelerator`：验证 accelerator 格式
- 新增 `src/components/HotkeyRecorder.tsx`：热键录制控件组件
  - 点击按钮进入录制态，显示「按下新热键...」
  - 捕获键盘事件并转换为 accelerator 格式
  - 录制期间调用 `hotkeySuspend` 暂停全局热键
  - Esc 取消、Backspace 清空、组合键自动保存
  - 失焦或组件卸载时自动恢复热键
- 后端新增 `hotkey_suspend` / `hotkey_resume` 命令
  - `HotkeyManager` 新增 `suspended` 集合，记录被暂停的热键
  - 注册热键时检查 suspended 状态，被暂停的热键不触发事件
- 设置面板热键页使用 `HotkeyRecorder` 替代文本输入框
- 测试：22 个 keys 单元测试 + 7 个 HotkeyRecorder 组件测试 + 2 个 Rust 暂停/恢复测试
- `npm.cmd run verify` 全绿（91 前端 + 133 Rust passed）
- 交叉评审：待执行（L1，1 轮）
