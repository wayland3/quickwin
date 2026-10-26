---
name: Release Check
description: QuickWin 发布前检查：版本一致性、质量门、打包产物、干净环境冒烟与回滚准备。在准备安装包或发布新版本时使用。
---

# 发布检查

## 触发条件

- 用户说「打包」「发版」「发布 vX.Y.Z」「给别人试用」。
- M7 阶段任务。

发布是**不可逆**动作（用户机器上会留下安装记录与数据），每一步失败都要停下来报告，不允许「先发出去再说」。

## 1. 版本一致性

```powershell
node -e "const p=require('./package.json');console.log(p.version)"
# Cargo.toml 的 version
Select-String -Path src-tauri/Cargo.toml -Pattern '^version'
# tauri.conf.json 的 version
node -e "console.log(require('./src-tauri/tauri.conf.json').version)"
```

三处必须完全相同，且符合 semver：

- 修复 → patch
- 兼容新增 → minor
- 破坏性变更（数据格式/热键默认值）→ major，并写迁移说明

版本号变更单独一个提交：`chore(release): vX.Y.Z`。

## 2. 质量门

```powershell
npm.cmd run verify
```

必须全绿，且：

- 没有新增 `#[ignore]`、`skip`、`todo!()`、`unwrap()`（`git diff` 自查）。
- 迁移测试覆盖最新版本。
- `TASKS.md` 中本版本包含的任务全部 `[x]`，阻塞项 `[!]` 已处理或明确不发布。

## 3. 数据兼容

- [ ] 从上一版本的真实数据库升级：备份 `quickwin.db` → 启动新版 → 数据完整、无报错。
- [ ] 降级测试：新版库用旧版打开会怎样？（预期：拒绝并提示，不损坏）
- [ ] 清空/重建路径可用：设置页「清空全部数据」后能正常重新初始化。

## 4. 打包

```powershell
npm.cmd run tauri:build
```

检查产物：

| 项 | 期望 |
|---|---|
| NSIS 安装包 | `src-tauri/target/release/bundle/nsis/*.exe`，≤15MB |
| 便携版 | 解压即用，不写注册表（除自启） |
| 图标 | 任务栏/开始菜单/托盘清晰，无白边 |
| 签名 | 未签名需在 release notes 说明 SmartScreen 提示 |
| WebView2 | 缺失时提示下载，不直接崩溃 |

## 5. 干净环境冒烟

在**未装过本项目**的 Win11（虚拟机或新用户）上：

- [ ] 安装 → 首次启动引导 → 接受默认热键
- [ ] `Alt+Space` 打开、输入 `chr` 能搜到应用
- [ ] `Alt+V` 粘贴一条刚复制的文本
- [ ] `Alt+X` 对时间戳做转换并替换
- [ ] 托盘菜单全部可用；退出后热键释放
- [ ] 开启开机自启 → 重启 → 托盘出现且不弹窗
- [ ] 卸载 → 询问是否删除数据 → 选择保留后重装，数据仍在
- [ ] 中文用户名、路径含空格、150% 缩放

另跑 `docs/WIN32-NOTES.md` 第 10 节完整清单。

## 6. 发布说明

```markdown
## vX.Y.Z (YYYY-MM-DD)

### 新增
- ...

### 修复
- ...

### 已知问题
- 管理员权限窗口不支持自动粘贴，需手动 Ctrl+V
- Alt+Space 可能与 PowerToys Run 冲突，可在设置中换绑

### 数据与升级
- 本版本无 schema 变更 / 升级后自动迁移，旧数据保留
```

## 7. 失败处理

| 失败点 | 处理 |
|---|---|
| verify 红 | 停止发布，修复或回退对应提交 |
| 打包失败 | 先在本机复现并记录完整日志（脱敏），不要反复重试 |
| 冒烟失败 | 停止发布；若已上传，撤回产物并通知试用者 |
| 数据迁移失败 | 立即停止；保留用户备份；在新分支修复后重新走全流程 |

## 8. 禁止

- 跳过冒烟直接发布。
- 手动修改 `target/` 里的产物（改配置后重新构建）。
- 把测试数据库、日志、`blobs/` 打进安装包。
- 在没有备份的情况下让用户升级带 schema 变更的版本。
