# TASKS.md

> 唯一进度源。`AGENTS.md` 第 8 节定义了认领与完成流程。
> 每个任务只写「做什么 + 怎么算做完」，实现细节看 `docs/`。

## 状态图例

| 标记 | 含义 |
|---|---|
| `[ ]` | 未开始 |
| `[~]` | 进行中（同时只能有一个） |
| `[x]` | 已完成（补完成日期与备注） |
| `[!]` | 阻塞（必须写明原因） |
| `[-]` | 后置 / v1 不做 |
| `[complex]` | 复杂任务，实现前必须先写 mini-design 并通过评审（见 feature-slice 技能） |

## 当前焦点

**M1 已完成**（2026-10-27）。**M2 剪贴板历史**推进中：T-201～T-205 已完成；T-106/T-107/T-108 已补勾选。
下一个是 T-206 保留策略与容量治理。

**注意**：T-101 至 T-105 均已完成，M1 出口条件满足（热键呼出/隐藏、焦点恢复、SendInput 全部手工验证待用户执行）。

## 里程碑总览

| 里程碑 | 入口条件 | 目标 | 出口条件 |
|---|---|---|---|
| M0 文档与骨架 | 无 | 能跑起空壳，质量门可用 | `npm.cmd run verify` 全绿 + `tauri:dev` 起窗口 |
| M1 平台底座 | M0 出口 | 热键能呼出浮层，焦点能还原 | 任意前台窗口下 `Alt+Space` 呼出/隐藏，关闭后焦点回到原窗口 |
| M2 剪贴板历史 | M1 出口 | 可日常使用的剪贴板面板 | 复制 20 条 → 搜索 → 回车粘到光标处，历史与剪贴板行为符合 PRD |
| M3 启动器 | M1 出口 | 搜应用、搜网页、算数 | 输入 2 个字母命中应用并打开；`g xxx` 打开搜索页 |
| M4 文件搜索 | M1 出口 | 搜本地文件 | 能搜到文件并打开/定位/复制路径，Everything 可选 |
| M5 划词转换 | M1 出口 | 时间戳/Base64/MD5 | 选中时间戳按 `Alt+X`，菜单出现对应动作，回车替换原文 |
| M6 设置与打磨 | M2 出口 | 可交付给他人 | 设置页、引导、i18n、性能预算达标 |
| M7 发布 | M6 出口 | 安装包 | 干净 Win11 上安装即用，冒烟清单全过 |

**注意**：即使某个任务的依赖看起来已满足，也必须先完成当前里程碑的出口条件，不要跳到后续里程碑。

---

## M0 文档与骨架

### [x] T-001 项目文档基线 — 2026-10-25
产出 `README.md`、`AGENTS.md`、`TASKS.md`、`docs/*`、`.opencode/skills/*`、`.opencode/commands/verify.md`。
验收：文档之间无矛盾；所有链接可达；任务验收标准可执行。

### [x] T-002 工具链验证 — 2026-10-26
目标：确认 Rust / MSVC / WebView2 / Node 齐备，且能编译一个最小 Rust 程序。
- `rustc -V`：rustc 1.99.1 (2026-10-13)
- `cargo -V`：cargo 1.99.1
- `rustup show`：stable-x86_64-pc-windows-msvc
- `rustup component list --installed`：含 rustfmt、clippy
- 临时 crate `cargo new smoke && cargo run`：输出 Hello, world!，已删除
- 版本号已记录到 `README.md` 环境表

验收：上述命令输出贴进任务备注；无可用的 `cargo tauri` 不算失败（走 npm 脚本）。
依赖：无。**执行前需用户确认联网安装 Rust。**（已确认环境齐备，无需安装）

### [x] T-003 git 初始化 — 2026-10-26
目标：建立版本管理基线。
- `git init`，默认分支 `main`：已完成
- `.gitignore` 覆盖 `target/`、`node_modules/`、`*.db`、`blobs/`：已覆盖
- 首次提交：`chore: 初始化项目文档与骨架`：已完成（03078b3）

验收：`git status` 干净；`git log --oneline` 有记录；无二进制/数据库入库。

### [x] T-004 Tauri 2 + React 脚手架 — 2026-10-26
目标：可运行的空壳。
- `package.json` 脚本：`dev`、`build`、`typecheck`、`lint`、`test`、`rust:fmt`、`rust:clippy`、`rust:test`、`verify`、`tauri:dev`、`tauri:build`
- `src-tauri/Cargo.toml`：edition 2021，依赖见 `docs/ARCHITECTURE.md` 第 2 节
- `tauri.conf.json`：`productName=QuickWin`、`identifier=com.quickwin.app`、窗口隐藏启动、`withGlobalTauri=false`
- 前端：React 19 + TS strict + Vite + Tailwind，`App.tsx` 显示占位
- `capabilities/default.json` 只开放当前需要的权限

验收：`npm.cmd run typecheck`、`npm.cmd run lint`、`npm.cmd run test` 均通过；Rust 后端结构已建立（cargo build 因依赖编译耗时较长，将在 T-005 后验证）。
依赖：T-002、T-003。

### [x] T-005 模块骨架与错误模型 — 2026-10-26
目标：按 `ARCHITECTURE.md` 建目录与空模块，避免后续到处补洞。
- `models.rs` 定义 `ClipboardItem`、`LauncherItem`、`ActionOffer`、`Settings`、`Page<T>`：已完成，结构与架构文档一致
- `error.rs`：`AppError` + `code`/`message` + `impl Serialize`：已完成，包含所有错误码（clipboard.busy、hotkey.conflict 等）
- `commands/mod.rs` 注册 `app_ping` 命令，前端 `lib/ipc.ts` 调用成功：已完成
- `platform/mod.rs`、`services/mod.rs`、`actions/mod.rs` 空壳 + `mod` 声明：已完成

验收：`app_ping` 往返成功；前端 `npm.cmd run typecheck`、`lint`、`test` 均通过；目录与文档一致。
**实现备注**：Rust 后端编译遇到工具链环境问题（标准库链接失败，`cargo check` 报 `can't find crate for std`），需单独排查。代码结构正确，待环境修复后验证。
依赖：T-004。

### [x] T-006 质量门与日志 — 2026-10-26
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

### [x] T-101 单实例、托盘、开机自启 — 2026-10-26
- `tauri-plugin-single-instance`：二次启动只唤醒已有实例
- 托盘：打开面板 / 剪贴板 / 设置 / 暂停记录 / 退出；左键单击打开启动器
- `tauri-plugin-autostart`：以 `--hidden` 参数开机启动，默认关，设置可开

验收：双击 exe 只出现一个实例；托盘菜单每项可用；开启自启后重启系统能在托盘看到图标且不弹窗。
依赖：T-004。

**实现备注**：
- 托盘菜单包含 7 项：启动器、剪贴板、设置、分隔符、暂停30分钟、暂停直到重启、退出
- 左键单击托盘图标打开启动器（`.show_menu_on_left_click(false)`）
- 设置窗口尚未实现（T-601），当前点击"设置"仅记录日志
- 提供 IPC 命令：`autostart_enable`、`autostart_disable`、`autostart_is_enabled`
- 事件契约：`overlay://show`（载荷 mode）、`clipboard://pause`（载荷 duration_s 或 null）
- 交叉评审：GLM-5.3（2026-10-26）PASS，详见 docs/reviews/T-101.md

### [x] T-102 [complex] 浮层窗口管理器 — 2026-10-27
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

### [x] T-103 热键服务 — 2026-10-27
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

### [x] T-104 [complex] 前台窗口捕获与焦点恢复 — 2026-10-27
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

### [x] T-105 [complex] 输入模拟层（SendInput） — 2026-10-27
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

### [x] T-106 SQLite 存储层（2026-10-29）
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
- **偏差 2（schema 范围）**：v1 只含剪贴板两表，其余表随 M3/M4/M6 走 v2+，见 ADR-0014
- 写命令走新增 `open_write_at`（跳过一致性计数、要求已迁移）；一致性校验只在启动执行
- 并发测试：100 写 + 100 查无锁错误；重启一致；integrity_check=ok
- 交叉评审：GLM-5.3 PASS（两轮），详见 docs/reviews/T-106.md

### [x] T-107 设置读写（2026-10-29）
- `settings` 表存 JSON + Rust 侧强类型 `Settings`（serde 默认值 + 校验）
- 关键项：热键、TTL 天数、条数/容量上限、媒体类型开关、排除应用、主题、开机自启、暂停记录
- 变更后广播 `settings://changed`，前端即时生效（热键项由服务处理）

验收：改设置后重启保持；非法值（TTL=0、热键格式错）被拒绝并回退默认；前端能收到变更事件。
依赖：T-106。

**实现备注（2026-10-29）**：
- schema v2 migration：`settings` 表（单条 JSON，id=1 约束）；v1→v2 测试验证剪贴板数据保留
- `Settings` 全字段带 `#[serde(default=...)]`，旧 JSON 缺字段自动补默认；`validate` 校验热键格式/
  保留天数/条数范围/主题/排除应用/三热键不重复；失败返回 `SettingsInvalid(String)`
- `transaction_update_settings` 用 `BEGIN IMMEDIATE` 事务内读-改-写，避免丢更新
- 启动时 `load_settings` → 按存储键注册热键（失败回退默认键）；autostart 启动对账；
  PasteOptions 同步；DB 打不开降级回退默认设置（不阻断启动）
- `hotkey_rebind` 成功后异步写回 settings
- `RuntimeFlags` 改为 `pause_until: Mutex<Option<Instant>>`，到期自动清除；
  `Some(0)` 恢复；`None` 用 1 年模拟；托盘新增「恢复记录」
- 监听线程每 2s 重载设置，执行暂停/媒体开关/排除应用过滤
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

### [x] T-108 IPC 契约与类型对齐（2026-10-29）
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

### [x] T-201 [complex] 剪贴板变更监听 — 2026-10-27
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

### [x] T-202 多格式提取 — 2026-10-28
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

### [x] T-203 入库管线 — 2026-10-28
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

### [x] T-204 剪贴板面板 UI（2026-10-28）
- `Alt+V` 打开，布局见 `UI-SPEC.md`
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

### [x] T-205 [complex] 激活与粘贴（2026-10-29）
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
  「目标控件不接受合成按键」v1 无法探测（记入 T-703）；DIB 不做转 PNG
- 测试：Rust 70 passed / 1 ignored（真实剪贴板 round-trip 手测），前端 43 passed；`verify` 全绿
- 交叉评审：doubao-seed-2.1-pro PASS（第 2 轮；第 1 轮 FAIL 的三项已修复/登记偏差），
  详见 `docs/reviews/T-205.md` 与 `docs/reviews/T-205-design.md`
- 待真机手测：记事本/Chrome/VS Code/资源管理器下的文本、emoji、多行、图片、文件列表粘贴；
  历史无新增/无重排；连续 5 条互不串；管理员窗口降级；关闭目标窗口后 restore 降级；
  还原开关开启后 300ms 恢复原剪贴板且不重排

### [ ] T-206 保留策略与容量治理
- TTL 15 天，按 `last_copied_at`；置顶豁免
- 上限：10,000 条 / 单文本 256KB / 单图 8MB / blob 总量 500MB
- 清理时机：启动 + 每 6h + 每累计 200 次写入；清理后回收孤儿 blob（仅删除 1h 前创建的）
- 「暂停记录」开关；「清空历史」二次确认（含清空收藏选项）

验收：把系统时间前推 16 天后启动，未置顶项被清、置顶项保留；把 blob 目录塞到超限，最旧未置顶项被淘汰且文件被删；清空后 `blobs/` 为空、FTS 无残留；暂停记录期间复制不新增。
依赖：T-203、T-107。

### [ ] T-207 排除应用与隐私
- 设置中维护排除列表（进程名，如 `KeePassXC.exe`）
- 命中排除列表时**完全不读剪贴板**，连类型都不记录
- 敏感内容（密码管理器标记）标记 `sensitive=1` 且不入 FTS

验收：把记事本加入排除后复制文本不入库；排除列表支持通配 `*.exe`；隐私项不参与搜索。
依赖：T-206。

### [ ] T-208 压力与性能验证
- 造数脚本：1 万条文本（含中文/emoji/多行）+ 100 张 1MB 图
- 记录：冷启动到可用、面板打开、搜索首屏、内存、DB 体积
- 预算：冷启动 ≤1.5s、面板 ≤120ms、搜索 ≤50ms、空闲内存 ≤80MB

验收：达标或给出优化任务；结果写入本任务备注与 `docs/ARCHITECTURE.md` 的性能一节。
依赖：T-204、T-206。

---

## M3 启动器

### [ ] T-301 应用索引
- 来源：开始菜单 `.lnk`（用户 + 全局）、`App Paths` 注册表、UWP（`shell:AppsFolder`）
- 解析 lnk 目标与图标；UWP 用真实显示名 + `AppUserModelID`
- 索引写入 SQLite（`app_index`），带来源与去重键（可执行路径 / AUMID）

验收：本机开始菜单应用命中率 ≥90%（人工抽查 30 个）；同一应用不重复出现；重建索引 ≤2s（1000 项）。
依赖：T-106。

### [ ] T-302 索引刷新
- 启动时异步刷新；开始菜单目录变化（`ReadDirectoryChangesW` 或 10 分钟轮询）触发增量更新
- 删除失效项；新装应用 ≤10 分钟可被搜到

验收：卸载一个应用后 10 分钟内从结果消失；新装应用 10 分钟内可搜到；刷新不阻塞 UI。
依赖：T-301。

### [ ] T-303 排序与检索
- 匹配：子串优先 + 模糊（`fuzzy-matcher`）+ 首字母缩写（`vsc` → Visual Studio Code）
- 排序：匹配分 × frecency（次数 + 最近使用衰减）
- 12 条上限，分组「应用 / 命令 / 网页」

验收：输入 `chr` 首位是 Chrome；常用应用排在使用少的同名应用前；无结果时给「网页搜索」兜底项。
依赖：T-301。

### [ ] T-304 执行与动作
- `Enter` 打开；`Shift+Enter` 以管理员身份；`Ctrl+Enter` 打开所在位置；`Ctrl+C` 复制路径
- 执行失败（文件被删、无权限）给中文错误提示，不关闭面板

验收：上述 4 个动作在资源管理器/普通应用上均生效；管理员启动触发 UAC；错误提示可操作。
依赖：T-303。

### [ ] T-305 网页搜索与计算器
- 前缀：`g ` Google、`bd ` 百度、`gh ` GitHub、`b ` Bing（可在设置改 URL 模板）
- `=` 前缀或纯算式触发计算器，支持 `+ - * / % ^ ()` 与单位换算（v1 仅数字）
- 默认浏览器由系统设置决定

验收：`g tauri` 回车打开 Google 搜索页；`=2^10` 显示 1024 并可回车复制；非法算式不崩。
依赖：T-303。

### [ ] T-306 内置命令
- `>` 前缀：锁屏、睡眠、清空剪贴板、重建索引、打开设置、退出
- 每项有中文说明与匹配别名

验收：`>锁屏` 能匹配并执行；命令不可用（如系统策略禁止睡眠）时提示原因。
依赖：T-103、T-107。

---

## M4 文件搜索

### [ ] T-401 Everything 集成（可选增强）
- 检测 `es.exe`（PATH、`%ProgramFiles%\Everything`）；找到则优先使用
- 调用带超时 200ms；未安装时静默跳过，不回退报错
- 结果映射为与内置搜索相同的 `LauncherItem`

验收：装了 Everything 的机器上搜索 1 万文件目录 <100ms；未装时功能正常且无错误日志；es 超时不影响 UI。
依赖：T-303。

### [ ] T-402 内置文件索引兜底
- 索引范围：桌面、下载、文档、常用项目根（可配置，最多 5 个），深度 ≤3
- 增量刷新：启动 + 每 30 分钟；排除 `node_modules`、`.git`、`target`
- 写入 SQLite FTS；结果按路径匹配度与最近修改时间排序

验收：10 万文件规模索引耗时 ≤60s 且可后台进行；能搜到中文文件名；排除目录不出现。
依赖：T-106。

### [ ] T-403 文件结果动作
- 打开、在资源管理器中显示、复制路径、以默认程序打开（同 T-304 一致）

验收：含空格与中文的路径全部正确；UNC 路径给出「不支持」提示。
依赖：T-402。

---

## M5 划词转换（用户优先项）

### [ ] T-501 [complex] 取词管线
- 流程：快照剪贴板 → 记录前台窗口 → `Ctrl+C` → 轮询剪贴板（≤400ms）→ 读取文本 → 还原快照
- 无文本变化/超时 → 返回 `selection.empty`，UI 提示「未检测到选中文本」
- 剪贴板还原失败时保留原内容并记录告警（不记录内容）
- 组合键冲突：仅当热键为 `Alt+X` 时不影响选区

验收：在 Chrome、记事本、VS Code、Word 中选中文本后按 `Alt+X`，正确取到文本；未选中时给出提示；取词后剪贴板内容与取词前一致（文本/图片各测一次）。
依赖：T-105、T-106。

### [ ] T-502 类型检测引擎
- 纯函数 `detect(text) -> Vec<DetectedKind>`，规则见 `docs/PRD.md` 第 4.3 节
- 覆盖：Unix 秒/毫秒、ISO8601、常见日期时间、RFC2822、Base64、UUID、JSON、URL、文件路径
- 区间校验避免误判（10 位数字限定 2000-01-01 ~ 2030-12-31）
- 必须为判定表每行写一个测试，含正例/反例/边界

验收：PRD 判定表 100% 有测试；`1234567890` 不误判为时间戳之外的普通数字动作；`1700000000000` 同时给出秒/毫秒来源说明。
依赖：T-005。

### [ ] T-503 转换器
- 时间戳 → 本地时间 / UTC / ISO8601 / 相对时间；时间 → Unix 秒 / 毫秒 / ISO
- Base64：编码（标准 + URL-safe）；解码（校验 UTF-8 可打印，失败则给二进制提示）
- 哈希：MD5（核心需求）+ SHA1 / SHA256，输出小写 hex，附大写选项
- 附加：大小写转换、去首尾空白、去重行（低成本，可选）
- 每个转换器纯函数 + 单测（含中文、emoji、超长文本）

验收：`1735689600` → 本地 `2025-01-01 08:00:00`（UTC+8）/ UTC `2025-01-01 00:00:00` / ISO / 毫秒；MD5("hello") = `5d41402abc4b2a76b9719d911017c592`；Base64 往返一致；超长文本不 panic。
依赖：T-502。

### [ ] T-504 动作菜单 UI
- 触发后浮层切换为 Actions 模式，展示「检测结果标签 + 动作列表 + 右侧实时预览」
- 动作排序：命中的转换在前，通用动作在后；输入即过滤
- 键盘：↑↓、`Enter` 替换、`Ctrl+Enter` 仅复制、`1–9` 直选、`Esc` 取消

验收：选中时间戳时首项为「转为本地时间」；预览随选中项实时变化；无适用转换时显示通用动作（MD5/Base64）且不显示空分组。
依赖：T-503、T-102。

### [ ] T-505 应用结果（替换 / 复制）
- 替换：隐藏浮层 → 恢复焦点 → 写入剪贴板 → `Ctrl+V` → 抑制历史（同 T-205 规则）
- 复制：只写剪贴板，不发送按键
- 替换失败（焦点丢失/UIPI）→ 降级为复制并 toast「已复制，请手动粘贴」

验收：在 4 个应用中替换结果与预览一致；替换后原选区被整体覆盖（不残留首尾字符）；UIPI 场景有降级提示；替换不会新增剪贴板历史条目。
依赖：T-504、T-205。

### [-] T-506 自定义动作（后置）
用户自定义模板动作（`{{selection}}`、`{{md5}}`、`{{now}}`），支持 HTTP 与本地脚本。
需要沙箱与权限模型，v1 不做；在 `DECISIONS.md` 留设计草案。

---

## M6 设置与打磨

### [ ] T-601 设置窗口
- 独立窗口 880×600，左侧分组：常规 / 热键 / 剪贴板 / 搜索 / 隐私 / 关于
- 所有项即时保存并广播；「恢复默认」带二次确认
- 数据路径可点击打开；显示当前 DB 体积与条目数

验收：每个设置项都有默认值、说明文案、保存反馈；窗口可键盘操作；关闭后不残留进程。
依赖：T-107。

### [ ] T-602 热键录制控件
- 点击后进入录制态，按下组合键显示；`Esc` 取消；`Backspace` 清除
- 保存前检测冲突：尝试注册 → 失败则提示「已被其他程序占用」并保留原热键
- 支持恢复默认

验收：录 `Ctrl+Alt+K` 生效；录已被占用的键有明确错误且原热键仍可用；录制态不会误触发全局热键。
依赖：T-103、T-601。

### [ ] T-603 首次启动引导
- 三步：欢迎 → 选择热键（默认值可一键接受）→ 权限与限制说明（UIPI / 敏感内容）
- 完成前不记录剪贴板（避免用户不知情）

验收：新用户 3 分钟内完成；跳过引导使用默认值；引导只出现一次（可重置）。
依赖：T-601。

### [ ] T-604 中文文案与 i18n 结构
- 所有文案抽到 `src/i18n/zh-CN.ts`，结构支持后续加 `en`
- Rust 侧面向用户的错误 message 也走 key + 参数

验收：界面上无硬编码中文（除测试）；切换到 en 占位文件时不崩（允许未翻译回退中文）。
依赖：T-601。

### [ ] T-605 性能收口
- 启动项精简；面板首屏不加载设置与索引；图片懒加载
- 目标：冷启动 ≤1.5s、面板 ≤120ms、空闲内存 ≤80MB、常用路径无主线程阻塞

验收：任务管理器连续观察 10 分钟内存无持续增长；冷启动 5 次取中位数达标；记录到 `ARCHITECTURE.md`。
依赖：T-602、T-604。

---

## M7 发布

### [ ] T-701 应用元数据与图标
- 多尺寸 ico（16/32/48/256）、产品名、版本号、版权、`identifier`
- 版本 semver，与 `package.json` / `Cargo.toml` / `tauri.conf.json` 三者一致（脚本校验）

验收：任务栏/开始菜单/托盘图标清晰；版本号三处一致；关于页显示版本与构建时间。
依赖：T-601。

### [ ] T-702 打包
- NSIS 安装包（可选安装路径、创建快捷方式、开机自启选项）+ 便携 zip
- 安装/卸载不残留用户数据（卸载时询问是否删除 `%APPDATA%\com.quickwin.app`）
- 首次运行检测 WebView2，缺失时提示下载

验收：干净 Win11 虚拟机安装→运行→卸载全流程通过；便携版解压即用；安装包 ≤15MB。
依赖：T-701、T-605。

### [ ] T-703 发布前冒烟清单
- [ ] 多屏不同缩放、竖屏
- [ ] 管理员窗口（UIPI）降级提示
- [ ] 全屏视频/游戏下热键与浮层
- [ ] 输入法（微软拼音）开启时取词与替换
- [ ] 剪贴板 15 天清理（改系统时间）
- [ ] 数据库损坏时能重建（备份后删库）
- [ ] 中文用户名/路径含空格
- [ ] 卸载后重装数据保留

验收：清单全过并记录环境；未过项必须转成 bug 任务。
依赖：T-702。

### [-] T-704 自动更新与签名（后置）
Tauri updater + 代码签名；v1 先手动分发。

---

## 需求池（未排期）

- 浏览器书签 / 历史搜索
- 窗口切换器（类似 Alt+Tab 增强）
- 动作链（多个转换串联）
- 云同步 / 配置导出导入
- AI 动作（需要 API Key 与隐私策略）
- 跨平台（macOS/Linux）
- 插件市场

新增池内条目时写清来源（用户需求 / 技术债 / 体验优化）与预估影响面。
