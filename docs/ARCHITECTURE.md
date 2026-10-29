# 架构

## 1. 总览

```
┌──────────────────────────── WebView (React 19 + TS) ────────────────────────────┐
│  Launcher  │  Clipboard  │  Actions  │  Settings                                │
│                  lib/ipc.ts（唯一 invoke 入口）                                  │
└───────────────────────────────────┬──────────────────────────────────────────────┘
                                    │ Tauri IPC (command / event)
┌───────────────────────────────────┴──────────────────────────────────────────────┐
│ Tauri App (Rust)                                                                  │
│  commands/    参数校验、编排、返回 models                                         │
│  services/    storage · search · ranking · hotkey · config · tray · index         │
│  actions/     detect · timestamp · base64 · hash（纯函数，无 IO）                  │
│  platform/    input · clipboard · focus · apps · files（唯一 unsafe / Win32 出口） │
└──────────────┬───────────────────┬────────────────────┬──────────────────────────┘
               │                   │                    │
        SQLite (WAL)          blobs/ 文件            Win32 (windows crate)
```

依赖方向严格单向：`commands → services → platform`，`services → actions`，`actions` 不依赖任何 IO 层。

## 2. 依赖清单

版本以 `Cargo.toml` / `package.json` 为准，本文只列用途，避免文档与 lockfile 漂移。

### 2.1 Rust

| crate | 用途 | 备注 |
|---|---|---|
| `tauri` | 应用框架 | features: `tray-icon` |
| `tauri-plugin-global-shortcut` | 全局热键 | 注册失败可捕获 |
| `tauri-plugin-single-instance` | 单实例 | 二次启动唤起浮层 |
| `tauri-plugin-autostart` | 开机自启 | 带 `--hidden` |
| `tauri-plugin-opener` | 打开文件/URL | 走系统默认程序 |
| `tauri-plugin-log` | 文件日志 | 按天轮转，禁止内容入日志 |
| `windows` | Win32 API | 只用到的 feature 子集，见下 |
| `rusqlite` | SQLite | feature `bundled`（自带 SQLite，FTS5 可用） |
| `serde` / `serde_json` | 序列化 | — |
| `chrono` / `chrono-tz` | 时间与时区 | 只做转换，不引入本地化依赖 |
| `md-5` / `sha2` | 哈希 | 计算与去重共用 |
| `base64` | Base64 | 标准 + URL-safe |
| `regex` | 检测规则 | 预编译 `OnceLock` |
| `thiserror` | 错误定义 | — |
| `fuzzy-matcher` | 模糊匹配 | Skim 算法 |
| `walkdir` | 文件索引 | 带排除规则 |
| `winreg` | 注册表读取 | App Paths |
| `uuid` | 标识 | 命令 ID 等 |

`windows` crate 需要的 feature（按实际编译报错增补，不要全开）：
`Win32_Foundation`、`Win32_UI_WindowsAndMessaging`、`Win32_UI_Input_KeyboardAndMouse`、`Win32_System_DataExchange`、`Win32_System_Memory`、`Win32_System_Threading`、`Win32_System_Registry`、`Win32_Graphics_Gdi`、`Win32_UI_Shell`、`Win32_System_Com`、`Win32_UI_Accessibility`（UWP 解析备用）。

### 2.2 前端

`react`、`react-dom`、`@tauri-apps/api`、`zustand`、`clsx`、Tailwind CSS、`vite`、`typescript`、`vitest`、`@testing-library/react`、ESLint + Prettier。

不使用 UI 组件库；不使用路由库（窗口内用 mode 状态切换）。

## 3. 进程与线程模型

| 执行体 | 职责 | 约束 |
|---|---|---|
| Tauri 主线程 | 窗口创建、事件循环、托盘 | 禁止阻塞；Win32 消息只在这里或专用线程 |
| Tokio runtime | `async` command | 禁止长时间持锁；阻塞调用走 `spawn_blocking` |
| 剪贴板监听线程 | 250ms 序列号轮询 → 读取 → 发 channel | 独立 `std::thread`；读取期间不 `await`；失败重试后跳过 |
| DB 写线程 | 独占写连接，串行执行写事务 | 通过 `mpsc` 收 `DbWrite`；定期 checkpoint |
| 索引线程 | 应用/文件索引构建 | 低优先级；支持取消；分片提交事务 |
| 热键回调 | 收到热键 → `app.emit` / 显示窗口 | 回调内不做重活，不做 IO |

**原则：一个资源只归一个线程。** 剪贴板句柄只在监听线程或 `spawn_blocking` 内打开；SQLite 写连接只在 DB 线程存在。

## 4. 窗口与模式

只有一个浮层窗口（`overlay`）+ 一个设置窗口（`settings`，按需创建）。

浮层通过 `mode` 状态切换内容：

| mode | 内容 | 触发 |
|---|---|---|
| `launcher` | 启动器 | `Alt+Space` |
| `clipboard` | 剪贴板历史 | `Alt+V` |
| `actions` | 划词动作菜单 | `Alt+X` |

这样做的理由：避免多窗口的焦点竞争与启动开销；高度是动态的，切换模式即切换尺寸。设置窗口独立，避免浮层失焦隐藏逻辑干扰。

窗口属性：`decorations=false`、`transparent=true`、`alwaysOnTop=true`、`skipTaskbar=true`、`visible=false`、`resizable=false`、`shadow=false`（阴影由 CSS 画）。显示前用 `current_monitor()` 计算目标显示器与位置。

## 5. 数据库

### 5.1 连接与 PRAGMA

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
PRAGMA foreign_keys = ON;
PRAGMA user_version;  -- 迁移版本
```

### 5.2 v1 schema

```sql
-- 键值元数据（索引版本、安装 id 等）
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- 剪贴板条目
CREATE TABLE clipboard_item (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  kind           TEXT    NOT NULL CHECK (kind IN ('text','html','rtf','image','files')),
  hash           TEXT    NOT NULL,
  preview        TEXT    NOT NULL,
  text_body      TEXT,
  html           TEXT,
  rtf            TEXT,
  blob_path      TEXT,
  thumb_path     TEXT,
  file_list      TEXT,              -- JSON: [{path,exists}]
  source_app     TEXT,
  source_title   TEXT,
  byte_size      INTEGER NOT NULL DEFAULT 0,
  created_at     INTEGER NOT NULL,  -- unix ms
  last_copied_at INTEGER NOT NULL,  -- unix ms，TTL 依据
  copy_count     INTEGER NOT NULL DEFAULT 1,
  pinned         INTEGER NOT NULL DEFAULT 0,
  sensitive      INTEGER NOT NULL DEFAULT 0,  -- 未使用（敏感内容过滤功能已移除）
  truncated      INTEGER NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX ux_clipboard_hash   ON clipboard_item(hash);
CREATE INDEX        ix_clipboard_recent ON clipboard_item(pinned DESC, last_copied_at DESC);
CREATE INDEX        ix_clipboard_kind   ON clipboard_item(kind, last_copied_at DESC);
CREATE INDEX        ix_clipboard_ttl    ON clipboard_item(pinned, last_copied_at);

-- 独立 FTS5（rowid = clipboard_item.id），不用触发器，写入时同步
CREATE VIRTUAL TABLE clipboard_fts USING fts5(
  preview,
  text_body,
  tokenize = 'trigram'
);

-- 应用索引
CREATE TABLE app_index (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  key        TEXT NOT NULL UNIQUE,   -- 小写 exe 路径或 AUMID
  name       TEXT NOT NULL,
  target     TEXT NOT NULL,
  args       TEXT,
  kind       TEXT NOT NULL CHECK (kind IN ('win32','uwp','url','command')),
  source     TEXT NOT NULL,          -- start_menu | app_paths | uwp | builtin
  icon_path  TEXT,
  updated_at INTEGER NOT NULL
);
CREATE INDEX ix_app_name ON app_index(name);

-- 文件索引（兜底搜索用；Everything 命中时不写这里）
CREATE TABLE file_index (
  id     INTEGER PRIMARY KEY AUTOINCREMENT,
  path   TEXT NOT NULL UNIQUE,
  name   TEXT NOT NULL,
  parent TEXT NOT NULL,
  size   INTEGER,
  mtime  INTEGER,
  root   TEXT NOT NULL
);
CREATE INDEX ix_file_name  ON file_index(name);
CREATE INDEX ix_file_mtime ON file_index(mtime DESC);

-- 使用频率（frecency）
CREATE TABLE usage_stat (
  item_key     TEXT PRIMARY KEY,
  kind         TEXT NOT NULL,        -- app | command | file
  use_count    INTEGER NOT NULL DEFAULT 0,
  last_used_at INTEGER NOT NULL
);

-- 设置（JSON 值，Rust 侧强类型校验）
CREATE TABLE settings (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);
```

### 5.3 关键 SQL

去重写入（同一 hash 只刷新，不新增）：

```sql
INSERT INTO clipboard_item (kind, hash, preview, text_body, byte_size, created_at, last_copied_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
ON CONFLICT(hash) DO UPDATE SET
  last_copied_at = excluded.last_copied_at,
  copy_count     = clipboard_item.copy_count + 1
RETURNING id, copy_count;
```

FTS 同步（独立表，显式 rowid）：

```sql
DELETE FROM clipboard_fts WHERE rowid = ?1;
INSERT INTO clipboard_fts(rowid, preview, text_body) VALUES (?1, ?2, ?3);
```

搜索（≥3 字走 FTS，1–2 字回退 LIKE；查询串必须用双引号包裹并转义内部引号）：

```sql
-- FTS
SELECT c.* FROM clipboard_fts f JOIN clipboard_item c ON c.id = f.rowid
WHERE clipboard_fts MATCH ?1
ORDER BY c.pinned DESC, bm25(clipboard_fts), c.last_copied_at DESC
LIMIT ?2 OFFSET ?3;

-- LIKE 回退
SELECT * FROM clipboard_item
WHERE preview LIKE ?1 ESCAPE '\' OR text_body LIKE ?1 ESCAPE '\'
ORDER BY pinned DESC, last_copied_at DESC
LIMIT ?2 OFFSET ?3;
```

TTL 清理（先取 id，再删 FTS，再删主表，同一事务；最后回收 blob）：

```sql
SELECT id, blob_path, thumb_path FROM clipboard_item
WHERE pinned = 0 AND last_copied_at < ?1;
```

容量淘汰：`ORDER BY pinned ASC, last_copied_at ASC` 逐条删到满足条数/blob 预算。

### 5.4 迁移

- `PRAGMA user_version` 为当前版本；`MIGRATIONS: &[fn(&Transaction) -> Result<()>]` 顺序执行。
- 每条迁移在独立事务内；失败回滚并保留原版本，启动时报错并提供「备份 + 重建」入口。
- 每条迁移必须配测试：用旧版 schema + 样本数据 → 迁移 → 断言版本与数据。
- 禁止修改已发布的迁移函数。

### 5.5 数据管理

- 库文件：`%APPDATA%\com.quickwin.app\quickwin.db`；附件：`blobs\<xx>\<hash>.<ext>`。
- 时间统一存 Unix 毫秒（`INTEGER`）；展示层再做本地化。
- 删除类操作（清空历史、删除条目）必须同步清理 FTS 行与 blob 文件。
- 迁移测试：内置 v1 空库 fixture；后续每个版本补一条「vN 数据 → 最新」测试。

## 6. IPC 契约

### 6.1 命令

Rust 侧 `snake_case`，前端调用同名。所有返回统一 `Result<T, AppError>`。

| 命令 | 入参 | 返回 | 说明 |
|---|---|---|---|
| `app_ping` | — | `{ version, build }` | 连通性检查 |
| `overlay_show` | `{ mode }` | `()` | 显示并聚焦（内部用） |
| `overlay_hide` | — | `()` | 隐藏并恢复焦点 |
| `overlay_resize` | `{ height }` | `()` | 120–560 夹取 |
| `launcher_query` | `{ query, limit }` | `{ items: LauncherItem[], hint? }` | 搜索应用/文件/命令 |
| `launcher_execute` | `{ key, action }` | `()` | action: `open`/`admin`/`reveal`/`copy_path` |
| `clipboard_list` | `{ query, filter, limit, offset }` | `{ items, total }` | 列表只返回 `preview`，不含全文 |
| `clipboard_get` | `{ id }` | `ClipboardDetail` | 全文/图片路径/文件列表 |
| `clipboard_activate` | `{ id, mode }` | `()` | mode: `paste`/`copy`；降级用 `app://toast` 提示 |
| `clipboard_set_pinned` | `{ id, pinned }` | `()` | — |
| `clipboard_delete` | `{ id, hard }` | `()` | hard 时立即删 blob |
| `clipboard_clear` | `{ include_pinned }` | `()` | 二次确认在前端做 |
| `clipboard_stats` | — | `{ count, pinned, blob_bytes, db_bytes }` | 设置页显示 |
| `selection_capture` | `{ timeout_ms? }` | `SelectionContext` | 取词 + 检测结果 |
| `selection_actions` | `{ text }` | `ActionOffer[]` | 不含 IO，可单测 |
| `selection_apply` | `{ action_id, text, mode }` | `ApplyResult` | mode: `replace`/`copy` |
| `settings_get` | — | `Settings` | — |
| `settings_update` | `{ patch }` | `Settings` | 校验失败返回 `settings.invalid` |
| `hotkey_status` | — | `{ bindings, failed }` | 设置页用 |
| `hotkey_rebind` | `{ action, accelerator }` | `{ ok, conflict? }` | 先注册新键成功再释放旧键 |
| `app_index_rebuild` | — | `{ count }` | 托盘/命令触发 |
| `file_index_rebuild` | `{ roots }` | `{ count }` | 设置页触发 |
| `diagnostics_export` | — | `{ path }` | 脱敏后导出，供反馈 |

### 6.2 事件

| 事件 | 载荷 | 时机 |
|---|---|---|
| `overlay://shown` | `{ mode }` | 浮层显示后 |
| `overlay://hidden` | — | 隐藏后 |
| `clipboard://changed` | `{ id, kind }` | 新条目入库或重复条目刷新（节流 500ms） |
| `clipboard://insert-error` | `{ code, message }` | 入库失败（不节流，前端提示后不崩） |
| `clipboard://stats` | `{ count, blob_bytes }` | 清理/删除后 |
| `hotkey://failed` | `{ action, accelerator, code }` | 注册失败 |
| `settings://changed` | `{ keys: string[] }` | 设置变更 |
| `index://progress` | `{ kind, done, total }` | 索引构建中 |
| `app://toast` | `{ level, message }` | 需要提示用户 |

事件**只传 id 与元数据，不传剪贴板内容**。

### 6.3 核心数据结构

```rust
pub struct ClipboardItem {
    pub id: i64,
    pub kind: ClipKind,           // text | html | rtf | image | files
    pub preview: String,
    pub source_app: Option<String>,
    pub created_at: i64,
    pub last_copied_at: i64,
    pub copy_count: i64,
    pub pinned: bool,
    pub sensitive: bool, // 未使用（敏感内容过滤功能已移除）
    pub truncated: bool,
    pub byte_size: i64,
    pub thumb_path: Option<String>,
}

pub struct LauncherItem {
    pub key: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub kind: LauncherKind,       // app | file | command | web
    pub icon: Option<String>,     // 本地图标路径；前端用 asset 协议加载
    pub score: f32,
}

pub struct ActionOffer {
    pub id: String,               // e.g. time.to_local
    pub group: String,            // 时间 / 编码 / 哈希 / 文本
    pub label: String,            // 中文
    pub preview: String,          // 结果预览（≤2000 字，超出标记）
    pub primary: bool,            // 默认选中
}

pub struct SelectionContext {
    pub text: String,
    pub kinds: Vec<DetectedKind>,
    pub source_app: Option<String>,
}
```

## 7. 关键流程

### 7.1 热键 → 显示浮层

```
hotkey callback
  → 读取目标 mode
  → 若已显示且同 mode：overlay_hide（切换语义）
  → 记录 foreground HWND（platform/focus）
  → 计算鼠标所在显示器与位置
  → window.show() + set_focus()
  → emit overlay://shown
  → 前端请求数据（clipboard_list / launcher_query 由前端输入触发）
```

注意：显示窗口会抢焦点，必须先记录前台窗口，再显示。

### 7.2 剪贴板捕获

```
loop every 250ms:
  seq = GetClipboardSequenceNumber()
  if seq == last_seq: continue
  sleep 30ms                       # 让源应用写完
  if paused or excluded(foreground_app): last_seq = seq; continue
  for attempt in 0..5:
      if OpenClipboard(owner): break
      sleep 20ms
  if failed: metrics.skip += 1; continue
  read formats（~~先查询敏感标记，命中则丢弃~~ 已移除）
  close
  build candidate → hash → send DbWrite::Put
DB 线程:
  upsert（重复只刷新）
  写 blob（如需要，先写临时文件再 rename）
  FTS 同步
  emit clipboard://changed（节流）
```

### 7.3 激活（粘贴）

```
clipboard_activate(id, mode):
  item = 读取完整内容（DB + blob）
  seq0 = 剪贴板序列号；suppress.begin(seq0)   # 先打开抑制窗口，避免 250ms 轮询抢跑
  overlay.hide_for_paste()                     # 同步隐藏 + 同步 restore，不走异步动画
  clipboard.write(item)                        # 单次会话写全部可写格式
  suppress.seal(seq0, seq1, item.hash)         # 记录精确区间 + hash 兜底，1.5s
  if mode == paste:
      input.send_ctrl_v()
      if restore 失败 / 提权: resume 面板 + app://toast("已复制，请手动粘贴")
```

顺序不能反：**先隐藏再恢复焦点**，否则焦点会被浮层抢回。

### 7.4 取词

```
selection_capture:
  snapshot = clipboard.snapshot()       # 读取所有可还原格式
  prev = focus.foreground()
  input.send_ctrl_c()
  for up to 400ms:
      if clipboard.seq changed and text != snapshot.text: break
      sleep 20ms
  text = clipboard.read_text()
  clipboard.restore(snapshot)           # 失败只记录告警
  detect(text) → action offers
```

剪贴板快照/还原的保真边界见 `WIN32-NOTES.md` 第 4 节。

## 8. 检索与排名

### 8.1 应用/命令

```
score = match_score(kind) × frecency × source_weight
match_score: 完全相等 1.0 | 前缀 0.9 | 词首缩写 0.85 | 子串 0.7 | 模糊 0.4–0.6
frecency   = 1 + ln(1 + use_count) × 0.5^(days_since / 14)
source_weight: start_menu 1.0 | app_paths 0.95 | uwp 0.9 | builtin 1.05
```

同分按标题长度升序、再按 key 字典序，保证结果稳定（测试需要）。

### 8.2 文件

`match_score` 只对文件名计算，路径命中给 0.5 系数；额外加权 `min(0.2, 1/(1+days_since_modified))`。结果去重按真实路径（大小写不敏感）。

## 9. 应用索引

| 来源 | 方式 | 备注 |
|---|---|---|
| 开始菜单 | 扫描 `%ProgramData%\Microsoft\Windows\Start Menu\Programs` 与 `%APPDATA%\...\Programs` 的 `.lnk`，解析目标与图标 | 用 `windows` COM `IShellLinkW` 或成熟 lnk 解析库（新增需 ADR） |
| App Paths | `HKLM/HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths` | 覆盖 Chrome、VS Code 等 |
| UWP | 枚举 `shell:AppsFolder`（`IShellItem` + `IApplicationActivationManager`） | 失败时降级为 `Get-StartApps` 快照，10 分钟过期 |
| 内置命令 | 代码内置清单 | 锁屏/睡眠/设置/清空剪贴板/重建索引/退出 |

刷新策略：启动后台构建；开始菜单变化用 10 分钟轮询（v1 不用 `ReadDirectoryChangesW`，降低复杂度）；构建过程分片提交，UI 可用旧索引。

## 10. 文件索引（兜底）

- 根目录来自设置（默认桌面/下载/文档），最多 5 个，深度 ≤3，单根条目上限 50 万。
- 排除规则集中在一个 `should_skip(path)` 函数，含 `node_modules`、`.git`、`target`、`AppData`、隐藏系统目录。
- 构建分片：每 2000 条提交一次；可取消（`AtomicBool`）。
- 与 Everything 并存时，结果合并后按路径去重。

## 11. 配置

`Settings` 为单一结构体（serde），持久化到 `settings` 表的一行 JSON 或多行 key；读取时对每个字段做范围校验，非法值回退默认并记录一条 warn（不含用户数据）。

默认值集中在 `services/config.rs::Settings::default()`，是唯一事实来源；`docs/PRD.md` 第 8 节必须与之一致。

## 12. 错误模型

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("剪贴板被其他程序占用，请稍后重试")]
    ClipboardBusy,
    #[error("无法读取剪贴板")]
    ClipboardRead,
    #[error("目标窗口权限较高，无法自动操作")]
    ElevatedTarget,
    #[error("未检测到选中文本")]
    NoSelection,
    #[error("文本过长（超过 256KB）")]
    TooLarge,
    #[error("热键 {0} 已被其他程序占用")]
    HotkeyConflict(String),
    #[error("数据库操作失败")]
    Storage(#[from] rusqlite::Error),
    #[error("内部错误")]
    Internal(String),
}
```

序列化为 `{ code, message }`，code 稳定，message 为中文可直接展示。`Internal` 只携带脱敏信息，详细堆栈进日志。

### 12.1 错误码注册表

新增错误码时必须更新本表，前端 `lib/ipc.ts` 的 toast 映射也依据本表。

| code | 含义 | 用户可见 message |
|------|------|------------------|
| `clipboard.busy` | 剪贴板被其他程序占用 | 「剪贴板暂时被占用，请稍后重试」 |
| `clipboard.read_failed` | 读取剪贴板失败 | 「无法读取剪贴板内容」 |
| `focus.elevated_target` | 目标窗口权限较高 | 「目标窗口权限较高，无法自动操作」 |
| `selection.empty` | 未检测到选中文本 | 「未检测到选中文本」 |
| `content.too_large` | 内容超过大小限制 | 「内容过长（超过 256KB）」 |
| `hotkey.conflict` | 热键被其他程序占用 | 「{热键} 已被其他程序占用」 |
| `hotkey.register_failed` | 热键注册失败 | 「热键注册失败，请尝试更换」 |
| `storage.error` | 数据库操作失败 | 「数据库操作失败」 |
| `settings.invalid` | 设置值不合法 | 「设置值不合法，已恢复默认」 |
| `internal` | 内部错误 | 「发生内部错误，请查看日志」 |

**规则**：
- `code` 用 `domain.specific_name` 格式，全小写下划线。
- `message` 简体中文，可直接展示给用户，不含技术细节。
- `internal` 类错误不暴露细节给前端，只记日志（脱敏）。

## 13. 日志与隐私

- 位置：`%APPDATA%\com.quickwin.app\logs\quickwin.YYYY-MM-DD.log`，保留 7 天。
- 级别：release 默认 `info`；`--verbose` 启动为 `debug`。
- 允许记录：命令名、耗时、条目 id、类型、长度、错误码、窗口类名。
- 禁止记录：剪贴板内容、窗口标题、文件路径、URL 查询串。
- `diagnostics_export` 导出前再过一遍脱敏白名单。

## 14. 性能策略

| 场景 | 手段 |
|---|---|
| 冷启动 | 浮层窗口预创建但隐藏；索引异步；数据库懒连接 |
| 面板显示 | 前端首帧只渲染骨架；数据一次查询 50 条；图片懒加载 |
| 搜索 | 输入防抖 80ms；应用索引常驻内存（≤1 万项）；剪贴板走 SQL + FTS |
| 内存 | 列表只持有 preview；全文按需 `clipboard_get` |
| 剪贴板轮询 | 只读序列号，无变化不打开剪贴板 |
| 数据库 | WAL + 批量事务 + 定期 `wal_checkpoint(TRUNCATE)` |

## 15. 测试策略

| 层 | 方式 | 必测内容 |
|---|---|---|
| `actions/` | `cargo test` 纯函数 | 判定表正例/反例/边界、时区、emoji、超长文本 |
| `services/storage` | 内存 SQLite | 去重、FTS 同步、TTL、容量淘汰、迁移 |
| `services/ranking` | 纯函数 | 排序稳定性、frecency 衰减 |
| IPC 契约 | 快照测试 | Rust 序列化字段集合 == TS 类型字段集合 |
| 前端 `lib/` | vitest | 键盘导航、过滤、错误映射 |
| `platform/` | 手动清单 | 见 `WIN32-NOTES.md` 第 10 节 |
| 迁移 | 每个 schema 版本一条 | 旧库 → 新库数据完整性 |

**测试命名**：用中文描述行为，例如 `fn 十位数字超出合理区间时不应识别为时间戳()`。

## 16. 目录与命名

见 `AGENTS.md` 第 5 节。新增模块必须同时更新该节，否则视为文档滞后。

## 17. 风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| `Alt+Space` 被占用 | 核心入口不可用 | 注册失败检测 + 一键换绑 + 备选键 |
| UIPI 限制 | 部分应用无法粘贴/取词 | 明确提示、降级为复制、文档说明 |
| 剪贴板格式还原不保真 | 取词后用户剪贴板被改 | 只还原可还原格式；失败告警；测试覆盖 5 类 |
| FTS5 trigram 对 1–2 字无效 | 中文短词搜不到 | 回退 `LIKE`；数据量小，性能可接受 |
| 图片占满磁盘 | 用户磁盘告警 | 500MB 预算 + LRU + 设置页可见 |
| 安全软件误报 | 无法安装/运行 | 不注入其他进程、不做键盘钩子、不写自启注册表（走系统 API） |
