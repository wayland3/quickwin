---
description: 运行 QuickWin 质量门（typecheck / lint / test / rust fmt / clippy / test）并输出报告
---

对当前改动运行 QuickWin 的质量门，逐项执行并把结果汇总成表格。

根据改动范围选择合适的质量门：

| 改动范围 | 命令 | 说明 |
|---------|------|------|
| 跨端改动 | `npm.cmd run verify` | 全量检查 |
| 仅前端（`src/`） | `npm.cmd run verify:fe` | typecheck + lint + test |
| 仅后端（`src-tauri/`） | `npm.cmd run verify:be` | rustfmt + clippy + rust test |
| 纯文档 | 不运行 | L0 自验即可 |

执行顺序（前一项失败也要把后面的跑完，便于一次看清全部问题）：

**前端**：
1. `npm.cmd run typecheck`
2. `npm.cmd run lint`
3. `npm.cmd run test`

**后端**：
4. `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
5. `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
6. `cargo test --manifest-path src-tauri/Cargo.toml`

注意：

- 本机 PowerShell 禁止 `npm.ps1`，必须用 `npm.cmd`。
- 如果脚本尚未创建（M0 未完成），明确说明「脚手架未就绪」，不要伪造结果。
- `package.json` 里若有 `verify` / `verify:fe` / `verify:be` 脚本，也可直接运行，但仍需分项报告。

输出格式：

| 检查 | 结果 | 关键信息 |
|---|---|---|
| typecheck | 通过 / 失败 / 跳过 | 错误数与首个错误位置 |
| lint | 通过 / 失败 / 跳过 | 规则名与文件 |
| 前端测试 | 通过 / 失败 / 跳过 | 通过数/总数，失败用例名 |
| rustfmt | 通过 / 失败 / 跳过 | 需要格式化的文件 |
| clippy | 通过 / 失败 / 跳过 | lint 名与文件:行 |
| rust 测试 | 通过 / 失败 / 跳过 | 通过数/总数，失败用例名 |

最后给出结论：**可以提交** 或 **不可提交**，并为每个失败项给出最小修复建议（改哪个文件、改什么），不要直接改代码，除非用户要求继续修复。

附加检查（若本次改动涉及）：

- 涉及 schema：是否新增 migration + 迁移测试。
- 涉及 `platform/`：是否记录了 Win32 手测结果。
- 涉及 UI：是否有空态/错误态。
