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
5. **剪贴板与热键建议单入口**：优先使用 `platform/clipboard` 和 `services/hotkey`。特殊场景（如抑制监听、临时查询序列号）可直接调用 Win32 API，但需在代码注释中说明理由。
6. **不记录剪贴板内容到日志**（包括 debug 级）。日志只允许出现 id、类型、长度、耗时。
7. **不擅自新增依赖**。新增 crate / npm 包需在 `TASKS.md` 实现备注中说明理由（1-2 句），并确认标准库或既有依赖无法解决。无需写 ADR，除非涉及架构变更。
8. **不修改验收标准**。任务做不完就拆任务或标记阻塞，不准把验收条件改松。
9. **一次只做一个任务**（`feature-slice` 技能）。禁止顺手重构无关模块。
10. **所有面向用户的文案用中文**，标识符、命令、日志字段用英文。
11. **提交前必须按评审级别完成交叉评审**：评审分三级，级别标注在 `TASKS.md` 任务条目前（`[L0]` / `[L1]` / `[L2]`，无标记默认 L1）。

    | 级别 | 适用场景 | 要求 |
    |------|---------|------|
    | **L0 轻量** | 纯文档、测试补全、文案修改、配置调整 | 实现者自验 + `verify` 全绿即提交，无需外部评审 |
    | **L1 标准** | 单模块业务逻辑（存储、查询、设置、UI 面板） | 1 轮外部评审；FAIL 只修 BLOCKER，NONBLOCK 登记遗留即可提交 |
    | **L2 严格** | 跨模块交互、Win32 边界、安全相关、`[complex]` 任务 | 设计评审 + 实现评审，最多 2 轮实现评审 |

    评审规则：
    - 评审意见必须标注 `[BLOCKER]`（运行时崩溃/数据丢失/安全漏洞）或 `[NONBLOCK]`（风格/优化/防御性改进）
    - **最多 2 轮**：第 1 轮全量审查，第 2 轮只验证 BLOCKER 修复；第 2 轮发现新问题 → 登记遗留，不打回（除非数据丢失/安全级别）
    - 评审模型固定使用 `zai-org/glm-5`，减少模型间标准差异
    - 评审记录写入 `docs/reviews/T-<id>.md`，并在 `TASKS.md` 实现备注中引用
    - 纯 Markdown 文档与代码注释的改动为 L0，汇报时写明「L0 自验」
    - 评审 FAIL 或无法执行 → 任务标 `[!]`，禁止提交

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

**环境自检**：运行 `scripts/check-env.ps1` 一键检查所有依赖。

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
| 质量门（全量）   | `npm.cmd run verify`                                                           |
| 质量门（仅前端） | `npm.cmd run verify:fe`                                                        |
| 质量门（仅后端） | `npm.cmd run verify:be`                                                        |
| 打包             | `npm.cmd run tauri:build`                                                      |
| 查看数据库       | 用 DB Browser for SQLite 打开 `%APPDATA%\com.quickwin.app\quickwin.db`（只读） |

`verify` 的定义：`typecheck` + `lint` + `test` + `rust:fmt --check` + `rust:clippy` + `rust:test`。**提交前必须全绿。**

## 5. 目录结构

```
quickwin/
├── AGENTS.md                  # 本文件
├── TASKS.md                   # 任务与进度（唯一进度源）
├── docs/                      # 规格、架构、编码规范、平台笔记、UI、ADR
├── scripts/                   # 环境检查等脚本
├── .opencode/skills/          # feature-slice / tauri-win32
├── .opencode/commands/verify.md
├── src/                       # React 渲染层
│   ├── main.tsx
│   ├── App.tsx                # 按 mode 渲染 Launcher / Clipboard
│   ├── windows/               # 两个浮层模式 + 设置窗口
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
│       └── platform/          # Win32 唯一出口：input / clipboard / focus / apps
└── tests/                     # 前端集成测试（vitest）
```

依赖方向：`commands → services → platform`。循环依赖视为设计错误。

## 6. 定义完成（DoD）

一个任务只有同时满足以下条件才算完成：

- [ ] 验收标准逐条可复现
- [ ] `npm.cmd run verify` 全绿（按改动范围可选 `verify:fe` / `verify:be`，见 §4）
- [ ] 新增/变更逻辑有对应测试
- [ ] 涉及 Win32 的，**列出**手测清单（应用、场景、预期结果），记录在 `TASKS.md` 实现备注
- [ ] 无新增 `unwrap` / `any` / 裸 `invoke`
- [ ] 用户可见文案为中文，错误提示可操作
- [ ] `TASKS.md` 勾选并写明完成日期；有偏差的写入「实现备注」
- [ ] 没有把无关改动混进本次提交
- [ ] 已按任务评审级别完成交叉评审（L0 自验 / L1 1 轮 / L2 设计+实现），评审记录在 `docs/reviews/T-<id>.md`

## 7. 已知限制（不要当 bug 去修）

1. **UIPI**：无法向管理员权限窗口发送输入。只能提示用户「目标窗口权限较高」。
2. **热键占用**：`Alt+Space` 可能被 PowerToys Run / 输入法占用。处理方式是可检测、可换绑、有提示，而不是换用更底层的键盘钩子（v1 不做）。
3. **剪贴板富格式保真**：跨应用还原 HTML/RTF 时格式可能丢失；v1 只保证文本、图片、文件列表可用。
4. **首次构建慢**：`rusqlite` bundled 首次编译 30–60s，属正常现象。

## 8. 任务工作流

详见 `feature-slice` 技能。核心循环：**选任务 → 读规格 → 实现+测试 → verify → 交叉评审 → 回写 TASKS.md → 汇报**。

阻塞时标 `[!]`，写清原因与需要的决策，然后停下来问，不要猜。

## 9. 文档地图

| 想知道                     | 看                         | 何时必读                     |
|----------------------------|----------------------------|------------------------------|
| 功能行为 + UI 规格         | `docs/SPEC.md`             | 实现该功能前                 |
| 模块分层、IPC 与表结构     | `docs/ARCHITECTURE.md`     | 首次进入项目 + 改架构时      |
| 编码规范与示例             | `docs/CODING-STANDARDS.md` | **写代码前**                 |
| Win32 怎么写、有哪些坑     | `docs/WIN32-NOTES.md`      | 碰 `platform/` 前            |
| 为什么这么选               | `docs/DECISIONS.md`        | 新增依赖 / 改架构前          |
| 现在做到哪了               | `TASKS.md`                 | 每次会话开始                 |

**上下文预算**：不要一次全读。只读当前任务相关的章节，feature-slice 技能第 2 步有具体指引。

## 10. 禁止事项

- 禁止把剪贴板内容、窗口标题、文件路径写进日志或错误上报。
- 禁止在未确认的情况下执行 `winget install`、修改系统执行策略、写注册表启动项（自启必须走 Tauri autostart 插件）。
- 禁止删除或跳过失败的测试来让 `verify` 变绿。
- 禁止在 `main` 分支直接提交未经 `verify` 的代码。
- 禁止提交 `quickwin.db`、`blobs/`、`target/`、`node_modules/`。
- 禁止在没有 migration 的情况下手工改用户库。

## 11. 提交规范

Conventional Commits，type 用英文，subject 用中文：

```
feat(clipboard): 支持图片历史与缩略图
fix(hotkey): 注册失败时提示可换绑的备选热键
docs(tasks): 完成 T-206 保留策略验收
test(actions): 补充时间戳边界用例
```

一次提交只对应一个任务或一个明确的修复。

提交前必须同时满足：`verify` 全绿 + 按评审级别完成交叉评审（L0 自验记录 / L1/L2 的 `docs/reviews/T-<id>.md` 有 PASS 记录）。提交信息末尾附评审证据（trailer，不影响 Conventional Commits）：

```
feat(clipboard): 支持图片历史与缩略图

Reviewed-by: GLM5.3 (PASS, L1)
```

L0 任务无需 Reviewed-by trailer，在 TASKS.md 备注「L0 自验」即可。

## 12. 术语表

| 术语 | 定义 |
|------|------|
| 浮层 | 无边框置顶透明窗口，失焦自动隐藏；分 Launcher / Clipboard 两种模式 |
| 保留策略 | TTL + 条数/容量上限 + 清理时机的组合（见 T-206） |
| frecency | frequency + recency，综合使用次数与最近使用时间的排序指标 |
| 质量门 | `npm.cmd run verify`（全量）/ `verify:fe`（前端）/ `verify:be`（后端），包含 typecheck + lint + test + rustfmt + clippy + rust test |
| 交叉评审 | 由非实现模型按规格只读复核代码，分 L0/L1/L2 三级，产出 PASS/FAIL 结论，是提交门禁 |
| BLOCKER | 评审中标注的阻塞项：运行时崩溃、数据丢失、安全漏洞，必须修复才能提交 |
| NONBLOCK | 评审中标注的非阻塞项：代码风格、性能优化等，登记遗留后可提交 |
| 脱敏 | 日志/导出中只保留 id、类型、长度，不保留内容（见 §10 禁止事项） |
| 单入口 | 剪贴板走 `platform/clipboard`，热键走 `services/hotkey`，禁止业务代码直接调用 |
| DoD | Definition of Done，定义完成清单（见 §6） |

## 13. 决策权限

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
| 跳过或放宽交叉评审门禁（如 L1 降 L0） | ❌ 需用户明确同意 |
| 执行系统级操作（winget / 注册表 / 执行策略） | ❌ 需用户明确确认（见 §10） |

**原则**：实现层自由，架构层谨慎，用户可见行为不可擅自变更。
