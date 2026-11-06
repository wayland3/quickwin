# Quick

跨平台常驻效率工具：`Alt+Space`（macOS: `Option+Space`）启动器、`Alt+V`（macOS: `Option+V`）剪贴板历史。

> 当前状态：**M0 文档与骨架阶段**。文档已定稿，业务代码尚未开始。进度以 [`TASKS.md`](./TASKS.md) 为唯一准绳。

## 功能

| 模块 | 说明 | 默认热键 |
|---|---|---|
| 启动器 | 搜索并打开应用 | `Alt+Space`（macOS: `Option+Space`） |
| 剪贴板历史 | 文本 / 图片 / 文件列表，搜索、置顶、回车粘贴 | `Alt+V`（macOS: `Option+V`） |
| 设置 | 热键录制、保留策略、主题 | 托盘菜单 |

所有热键都可在设置中改绑；注册失败时会有明确提示，不会静默失效。

## 技术栈

- **Tauri 2**（Rust 后端 + WebView 前端），安装包小、常驻内存低
- **React 19 + TypeScript + Vite + Tailwind CSS**
- **SQLite**（`rusqlite`，bundled + FTS5）存剪贴板历史、应用使用频率、设置
- **平台特定 API**：
  - Windows: `windows` crate（全局热键、`SendInput`、剪贴板监听、前台窗口与焦点）
  - macOS: `cocoa` / `core-graphics` crate
  - Linux: `x11` / `xcb` crate

选型理由与取舍见 [`docs/DECISIONS.md`](./docs/DECISIONS.md)。

## 环境要求

### 跨平台

| 依赖 | 版本 | 说明 |
|---|---|---|
| Windows 10 1809+ / macOS 12+ / Linux (X11/Wayland) | - | 主要目标 Windows 11、macOS 14+、Ubuntu 22.04+ |
| Node.js | ≥20（当前 v24.22.1） | 前端构建 |
| npm | ≥10（当前 11.19.1） | 包管理 |
| Rust stable | rustc 1.99.1+ | 后端编译 |
| Git | 2.55.0+ | 版本管理 |

### Windows 特定

| 依赖 | 版本 | 说明 |
|---|---|---|
| WebView2 Runtime | 155.0.4283.62+ | Win11 自带；Win10 需安装 Evergreen Runtime |
| MSVC Build Tools | VS 18 Build Tools | 需勾选「使用 C++ 的桌面开发」 |

> PowerShell 默认执行策略会拦截 `npm.ps1`，本项目所有脚本命令统一用 **`npm.cmd`**。

## 快速开始

```bash
npm install
npm run tauri:dev
```

Windows 下建议用 `npm.cmd` 替代 `npm`，避免 PowerShell 执行策略问题。

常用命令：

```bash
npm run dev          # 只启动前端（无 Tauri API，会报错，仅供样式调试）
npm run typecheck    # TypeScript 类型检查
npm run test         # 前端单测
npm run verify       # 质量门：typecheck + lint + test + rust fmt/clippy/test
npm run tauri:build  # 打包
```

Rust 侧单独跑：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

## 数据位置

### Windows
```
%APPDATA%\quick\
├── quick.db      # SQLite（剪贴板历史、设置、使用频率）
├── blobs\        # 图片/文件类剪贴板附件
└── logs\         # 运行日志（不记录剪贴板内容）
```

### macOS
```
~/Library/Application Support/quick/
├── quick.db
├── blobs/
└── logs/
```

### Linux
```
~/.local/share/quick/
├── quick.db
├── blobs/
└── logs/
```

剪贴板历史默认保留 **15 天**（按最后一次使用时间计算），置顶项豁免；容量与隐私规则见 [`docs/SPEC.md`](./docs/SPEC.md) §6.7。

## 已知限制

1. **管理员权限窗口**（Windows）：UIPI 限制导致无法向提权窗口发送模拟按键。这是系统级限制，非本项目的 bug。
2. **热键冲突**：`Alt+Space` 在部分机器上被 PowerToys Run 或输入法占用，注册失败时请到设置里换绑（备选 `Ctrl+Alt+Space`）。

## 文档

| 文件 | 内容 |
|---|---|
| [`AGENTS.md`](./AGENTS.md) | AI 协作总纲：硬规则、命令、目录、定义完成 |
| [`TASKS.md`](./TASKS.md) | 里程碑与任务清单（唯一进度源） |
| [`docs/SPEC.md`](./docs/SPEC.md) | 功能规格与 UI 规范 |
| [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) | 架构、IPC 契约、数据库 schema |
| [`docs/PLATFORM-NOTES.md`](./docs/PLATFORM-NOTES.md) | 平台特定 API 坑位与实现要点 |
| [`docs/DECISIONS.md`](./docs/DECISIONS.md) | 架构决策记录（ADR） |
