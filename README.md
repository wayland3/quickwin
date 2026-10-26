# QuickWin

Windows 上的快捷键效率工具，目标是做一个轻量、可自定义、贴近 Windows 使用习惯的 Alfred 替代品。

> 当前状态：**M0 文档与骨架阶段**。文档已定稿，业务代码尚未开始。进度以 [`TASKS.md`](./TASKS.md) 为唯一准绳。

## 功能

| 模块 | 说明 | 默认热键 |
|---|---|---|
| 启动器 | 搜索并打开应用、文件；支持网页搜索关键字与计算器 | `Alt+Space` |
| 剪贴板历史 | 文本 / 图片 / 文件列表，搜索、置顶、回车粘贴 | `Alt+V` |
| 划词转换 | 选中文本按热键，菜单里做时间戳↔时间、Base64、MD5 等转换 | `Alt+X` |
| 设置 | 热键录制、保留策略、排除应用、主题 | 托盘菜单 |

所有热键都可在设置中改绑；注册失败时会有明确提示，不会静默失效。

## 技术栈

- **Tauri 2**（Rust 后端 + WebView2 前端），安装包小、常驻内存低
- **React 19 + TypeScript + Vite + Tailwind CSS**
- **SQLite**（`rusqlite`，bundled + FTS5）存剪贴板历史、应用使用频率、设置
- **`windows` crate** 直接调用 Win32：全局热键、`SendInput`、剪贴板监听、前台窗口与焦点

选型理由与取舍见 [`docs/DECISIONS.md`](./docs/DECISIONS.md)。

## 环境要求

| 依赖 | 版本 | 说明 |
|---|---|---|
| Windows 10 1809+ / Windows 11 | 当前 10.0.26200 (Win11 24H2) | 主要目标 Windows 11 |
| WebView2 Runtime | 155.0.4283.62 | Win11 自带；Win10 需安装 Evergreen Runtime |
| Node.js | v24.22.1（≥20） | 前端构建 |
| npm | 11.19.1（≥10） | 包管理 |
| Rust stable | rustc 1.99.1 (2026-10-13) | `winget install Rustlang.Rustup` |
| Cargo | 1.99.1 | 随 Rust 安装 |
| MSVC Build Tools | VS 18 Build Tools | 需勾选「使用 C++ 的桌面开发」 |
| Git | 2.55.0.windows.3 | 版本管理 |

> PowerShell 默认执行策略会拦截 `npm.ps1`，本项目所有脚本命令统一用 **`npm.cmd`**。

## 快速开始

```powershell
npm.cmd install
npm.cmd run tauri:dev
```

常用命令：

```powershell
npm.cmd run dev          # 只启动前端（无 Tauri API，会报错，仅供样式调试）
npm.cmd run typecheck    # TypeScript 类型检查
npm.cmd run test         # 前端单测
npm.cmd run verify       # 质量门：typecheck + lint + test + rust fmt/clippy/test
npm.cmd run tauri:build  # 打包
```

Rust 侧单独跑：

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

## 数据位置

```
%APPDATA%\com.quickwin.app\
├── quickwin.db      # SQLite（剪贴板历史、设置、使用频率）
├── blobs\           # 图片/文件类剪贴板附件
└── logs\            # 运行日志（不记录剪贴板内容）
```

剪贴板历史默认保留 **15 天**（按最后一次使用时间计算），置顶项豁免；容量与隐私规则见 [`docs/PRD.md`](./docs/PRD.md)。

## 已知限制

1. **管理员权限窗口**：Windows 的 UIPI 限制导致无法向提权窗口发送模拟按键，也无法从中取词。这是系统级限制，非本项目的 bug。
2. **热键冲突**：`Alt+Space` 在部分机器上被 PowerToys Run 或输入法占用，注册失败时请到设置里换绑（备选 `Ctrl+Alt+Space`）。
3. **取词方式**：通过「模拟 Ctrl+C → 读取剪贴板 → 还原」实现，极少数应用（如部分游戏、远程桌面）不可用。

## 文档

| 文件 | 内容 |
|---|---|
| [`AGENTS.md`](./AGENTS.md) | AI 协作总纲：硬规则、命令、目录、定义完成 |
| [`TASKS.md`](./TASKS.md) | 里程碑与任务清单（唯一进度源） |
| [`docs/PRD.md`](./docs/PRD.md) | 功能规格与验收标准 |
| [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) | 架构、IPC 契约、数据库 schema |
| [`docs/WIN32-NOTES.md`](./docs/WIN32-NOTES.md) | Win32 平台坑位与实现要点 |
| [`docs/UI-SPEC.md`](./docs/UI-SPEC.md) | 浮层交互与视觉规范 |
| [`docs/DECISIONS.md`](./docs/DECISIONS.md) | 架构决策记录（ADR） |
