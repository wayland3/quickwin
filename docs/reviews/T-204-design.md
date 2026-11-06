# T-204 mini-design v5：剪贴板面板 UI

> 变更记录
> - v1 → FAIL（kimi-k3，5 个阻塞 B1–B5）
> - v2 → 逐条修复 B1–B5，复审请求中断未出结论
> - v3 → 用户拍板 3 个设计分歧（D1–D3）；补 5 项非阻塞建议（N1–N5）；复审模型改为 seed-2.1-pro
> - v4 → 第二轮复审 FAIL（seed-2.1-pro）：修复阻塞 B1（`protocol-asset` Cargo feature 缺失）；
>   采纳 S1–S3、S5、S6、S9–S12；S4/S7/S8 以「实现时遵循 + 记备注」方式落地
> - v5 → 第三轮复审 FAIL（seed-2.1-pro）：修复阻塞 N1（asset scope 双重 identifier → 403）、
>   N2（空库读路径可能报错而非空态）；采纳 T1（changed 刷新不把用户打回首屏）

## v4 阻塞修复：B1 `protocol-asset` feature

仅配置 `assetProtocol.enable=true` **不会**在编译期启用 asset 协议处理器：

- `tauri 2.12.5/src/manager/webview.rs` 的 `register_uri_scheme_protocol("asset", …)` 被
  `#[cfg(feature = "protocol-asset")]` 门控；`src/protocol/mod.rs` 的 `pub mod asset` 同样被门控。
- tauri crate 默认 features **不含** `protocol-asset`；项目当前为
  `tauri = { version = "2", features = ["tray-icon"] }`。
- `tauri dev` / `tauri build` 会由 CLI 读 `tauri.conf.json` 自动注入该 feature
  （`tauri-utils/config.rs` 的 `features()`），但 `cargo test`（`verify` 的 `rust:test`）
  与 IDE 不会注入 → 交付不可复现，且 `tauri-build` 的 manifest 校验会报 allowlist 不匹配。

**修复**：`src-tauri/Cargo.toml` 改为
`tauri = { version = "2", features = ["tray-icon", "protocol-asset"] }`；
配置项与 Cargo feature **二者缺一不可**。这不是新增 crate，属启用既有 crate 的 feature，
在 `docs/DECISIONS.md` 追加一段说明（不新开 ADR）。

## 0. 用户拍板的决策

| 编号 | 分歧 | 结论 |
|---|---|---|
| D1 | 搜索时置顶项内部排序 | **照 `ARCHITECTURE §5.3`**：`pinned DESC` 为第一键，置顶**集合**永远在最前，集合内部按 `bm25` 相关度排。不改规格。 |
| D2 | `Enter` / `Ctrl+Enter`（依赖 T-205） | **接线 + 明确「开发中」提示**。两键在 T-204 均有确定响应（底栏提示条），T-205 替换为真实动作。 |
| D3 | 回收站 | **v1 不做回收站**，`Delete` 与 `Shift+Delete` 都是物理删除（`hard` 参数区分是否删 blob）。记已知偏差，不改 schema。 |

## 目标

按 `docs/UI-SPEC.md` §4.3/§4.4/§5/§6/§9/§10 与 `docs/PRD.md` §6.3 实现剪贴板面板：
搜索防抖 80ms、Tab 切换筛选、纯键盘操作、右侧预览、空态/无结果/加载态齐全。

## R1（v1 评审 B1）删除语义（对应 D3）

- schema 无 `deleted_at` 列，v1 **没有回收站**。
- 命令签名照抄 `ARCHITECTURE §6.1` 既有契约 `clipboard_delete { id, hard }`：
  - `hard=false` → 删主表行 + FTS 行，**保留 blob 文件**（不提供恢复入口）
  - `hard=true` → 删主表行 + FTS 行 + 立即删 blob 文件
- **不改** §6.1 规格；「无回收站」写入 TASKS.md 实现备注。

## R2（v1 评审 B2）图片预览的 asset 协议链路

1. **后端返回绝对路径**：`ClipboardDetail.blob_path` = `data_dir.join(relative)`；前端 `convertFileSrc()` 转换。`data_dir` 逻辑不外泄。
2. **开启 asset 协议**（`src-tauri/tauri.conf.json`）：
   ```json
   "security": {
     "csp": null,
     "assetProtocol": { "enable": true, "scope": ["$APPDATA/blobs/**"] }
   }
   ```
   已验证：Tauri 2.12 `AssetProtocolConfig` 为 `{ scope, enable }`（camelCase）；`$APPDATA` 是 Tauri 支持的作用域变量；
   Tauri `app_data_dir()` = `dirs::data_dir()/identifier` = `quick`（**已含 identifier**），
   而 blob 落盘在 `quick/blobs/...`，所以 scope 只能是 `$APPDATA/blobs/**`。
   写成 `quick/blobs/**` 会双重 identifier → `is_allowed` 恒 false → 403（v4 复审阻塞 N1）。
3. **Cargo feature**：按 v4 阻塞修复 B1 开启 `protocol-asset`。
4. **capability 放行**：先按 `"core:default"` 实现，`tauri build` / dev 启动时用 Tauri 的 schema 校验；
   若报权限缺失，按报错补**最小**权限，不整包放行。已核实 `convertFileSrc()` 走
   `__TAURI_INTERNALS__.convertFileSrc`，**不需要**额外 capability 项。
5. `thumb_path` 当前恒为 NULL（T-203 偏差，WIC 未实现）：列表行用**类型占位图标**，预览区用 blob 原图（CSS 限宽 240×240）。
   `.dib` 源 WebView 不能解码 → **不渲染 `<img>`**（按扩展名/kind 先判断，避免必然失败请求的噪音），
   显示「该图片格式暂不支持预览」，不静默空白。
6. **禁止** base64 内联（UI-SPEC §10）。

## R3（v1 评审 B3）并发模型与受管状态

- `rusqlite::Connection` 是 `Send` 但**不是 `Sync`** → 不能 `app.manage(ClipboardStorage)`。
- **每条 command 开短生命期连接**（`ClipboardStorage::open_at(db_path)`），不共享 `Connection`。
  - WAL 多读一写；写冲突靠 `busy_timeout=5000` 兜底。
  - `ClipboardMonitor` 保持自己的长连接（写入路径不变）。
- `app.manage` 只放**不可变配置** `DbPath(PathBuf)`，command 用 `app.state::<DbPath>()` 取。
- 所有 SQL 在 `async fn` command 内走 `tauri::async_runtime::spawn_blocking`，**连接与锁都不跨 await**
  （`State<'_, DbPath>` 在 await 前先 clone 出 `PathBuf`）。
- **读路径轻量 open（S5）**：`open_at` 当前每次都执行 `migrate()` + `check_fts_consistency()`
  （两次 `COUNT(*)`）。为读路径新增 `open_readonly_at`：
  - `user_version == 0`（库从未迁移，例如监听线程初始化失败）→ **先执行 `migrate()` 再读**
    （migration 幂等），保证空库打开时是「空态」而不是 `storage.error` / `no such table`（v4 复审阻塞 N2）；
  - `user_version == SCHEMA_VERSION` → 只读打开，跳过 FTS 一致性全表计数；
  - `user_version > SCHEMA_VERSION` 或迁移失败 → 报 `storage.error`。
  `list` / `get` 用 `open_readonly_at`，`set_pinned` / `delete` 用完整 open。
- `set_pinned` / `delete` 与监听线程 `insert` 并发时，写冲突返回 `storage.error`（前端中文提示，不崩）。

## R4（v1 评审 B4）排序与 §5.3 对齐

- FTS：`ORDER BY c.pinned DESC, bm25(clipboard_fts), c.last_copied_at DESC, c.id DESC`
- 无搜索 / LIKE 回退：`ORDER BY pinned DESC, last_copied_at DESC, id DESC`
- `id DESC` 是新增稳定 tie-break；不改 §5.3 主排序键。

## R5（v1 评审 B5）输入法组合期

- 键盘处理统一先判 `e.nativeEvent.isComposing`；组合期只放行字符输入，导航/回车/Tab/Delete/PgUp/PgDn 一律不响应。
- 额外用 `compositionstart` / `compositionend` 维护 `isComposing` state 作双保险（部分 IME 的 `isComposing` 时序不可靠）。
- 搜索框 `onKeyDown` 与面板级 `onKeyDown` 都过闸。

## N1 分页加载策略

- 首屏 `limit=50, offset=0`；列表滚动到距底 ≤ 120px 且 `items.length < total` 时加载下一页（同一 query/filter）。
- query / filter 变化 → `offset=0` 重置并清空旧 items（避免旧结果与新条件混排）。
- **竞态（S3）**：每次条件变化递增 `generation`；每个请求带 `{ generation, offset }`。
  响应接受条件严格为 `generation === current && offset === expectedNextOffset`，
  否则**整条丢弃**。禁止「非 0 offset 一律追加」的写法——否则新条件的翻页响应先于旧条件首屏返回时，
  旧首屏会被追加成污染项。
- 追加后按 `id` 去重一次（防御后端在并发写期间的 offset 漂移）。
- 加载中：首屏用骨架屏；翻页用列表末尾 1 行骨架，不遮挡列表。
- `total` 以最新响应为准，`items.length >= total` 时停止翻页。

## N2 删除后的选中落点

- 删除第 `i` 项 → 选中原 `i+1`；若删除的是末项则选中 `i-1`；列表空则清空选中与预览区。
- 落点项若不在已加载页内（`i+1` 超出），补发一次 `offset = 0` 的刷新，避免预览区与选中不一致。
- 删除失败（`storage.error`）：底栏错误提示 + 保留原选中，不做乐观移除（先等后端成功再刷新）。
- `Shift+Delete` 与 `Delete` 走同一落点逻辑，仅 `hard` 不同。

## N3 复制次数徽标

- 列表行右侧显示 `×N`，**仅当 `copy_count >= 2`** 才显示（首次复制的条目无徽标，避免噪音）。
- `aria-label` 为「已复制 N 次」，不只用颜色/符号传达。

## N4 置顶分组分隔线

- 列表同时包含置顶项与非置顶项时，在**第一个非置顶项上方**渲染 1px `--border` 分隔线；只有一类时不渲染。
- 仅视觉分隔，不改变列表语义与键盘导航顺序（↑↓ 仍连续穿过分隔线）。

## N5 性能口径（验收「1000 条数据下输入到出结果 <50ms」）

- 防抖窗口（80ms，`ARCHITECTURE §5.2` 与 `PRD §6.3` 一致）**不计入**该 50ms：测量从「防抖触发」到「列表渲染完成」。
- 测量方法：dev 下 `console.time`/`performance.now()` 记录 `ipc.clipboardList` 往返 + 渲染；1000 条种子数据、FTS 3 字查询、LIKE 1 字查询各 10 次取中位数。
- **双口径披露（S4）**：A = 防抖回调发起 → DOM commit（`requestAnimationFrame` 后），验收以 A 为准；
  B = 最后一次按键 → DOM commit（含 80ms 防抖），仅作披露，不进验收。release 版各测一次作为参考。
- 后端只支持 `limit<=200`，首屏 50 条；`total` 用同条件 `COUNT(*)`。
- 若中位数 >50ms，先查索引/查询计划（FTS `rowid` 关联、`pinned/last_copied_at` 覆盖），再优化；**不通过放宽验收标准解决**。

## 改动文件

### 后端
- `src-tauri/src/services/storage.rs`：新增 `list` / `get` / `set_pinned` / `delete` + 查询条件构造
- `src-tauri/src/services/query.rs`（新增）：搜索条件与排序构造（纯函数，便于单测）
- `src-tauri/src/commands/clipboard.rs`（新增）：4 个 command
- `src-tauri/src/commands/mod.rs`：挂载
- `src-tauri/src/models.rs`：补 `ClipboardListQuery` / `ClipboardDetail` / `ClipboardListResult` / `ClipFilter`
- `src-tauri/src/lib.rs`：`app.manage(DbPath)`；挂载 command
- `src-tauri/Cargo.toml`：`tauri` features 增 `protocol-asset`（B1）
- `src-tauri/tauri.conf.json`：开 `assetProtocol`
- `src-tauri/capabilities/default.json`：按需补最小权限
- `docs/DECISIONS.md`：追加「启用 protocol-asset feature」一行说明（非新增依赖）

### 前端
- `src/lib/ipc.ts`：4 个带类型调用
- `src/lib/clipboard.ts`（新增）：纯函数（相对时间、筛选枚举循环、键盘动作映射、搜索有效性、徽标规则）
- `src/windows/ClipboardPanel.tsx`（新增）：面板容器（状态、防抖、竞态、分页、键盘）
- `src/components/ItemRow.tsx` / `PreviewPane.tsx` / `KeyHint.tsx` / `EmptyState.tsx` / `Skeleton.tsx` / `FilterChips.tsx`（新增，命名对齐 UI-SPEC §10）
- `src/App.tsx`：按 mode 渲染 ClipboardPanel
- `src/index.css`：配色 token + 面板样式
- `src/lib/clipboard.test.ts`、`src/components/*.test.tsx`：单测

## 数据流

```
Alt+V → overlay_show("clipboard") → App.tsx 渲染 ClipboardPanel
  ↓ 挂载
clipboard_list(query="", filter=All, limit=50, offset=0)
  ↓
后端：短连接 → ClipboardStorage::list → { items, total }
  ↓
选中项变化 → clipboard_get(id) → ClipboardDetail（requestId 丢弃过期响应）
  ↓
clipboard://changed（节流 500ms）→ 重新拉取当前 query/filter 的首屏
```

## 关键实现点

### 1. 搜索条件（services/query.rs，纯函数 + 单测）

```rust
pub enum SearchKind { None, Fts, Like }
pub fn build_search(query: &str) -> SearchSpec
```

判定顺序（S2：先判字符有效性，再判长度）：
1. `query.trim()` 为空 → `None`
2. 去首尾空白后**不含任何可搜索字符**（无字母、数字、汉字）→ `Like`
   （FTS trigram 对纯 ASCII 标点不产生有效 token，短语 `MATCH` 会报语法错误；
   全角标点行为在实现时用测试固定，不得静默报错）
3. `chars().count() >= 3` → `Fts`
4. 其余（1–2 字符）→ `Like`

- FTS 串：双引号包裹，内部 `"` → `""`
- LIKE：`ESCAPE '\'`，转义 `%` `_` `\`，模式用 `%...%` 包裹
- LIKE 同时匹配 `preview` 与 `text_body`

### 2. 列表查询

```rust
pub fn list(&self, query: &str, filter: ClipFilter, limit: u32, offset: u32)
    -> Result<ClipboardListResult, AppError>
```

- 筛选：`All` / `Text`（text/html/rtf）/ `Image` / `Files` / `Favorites`（pinned=1）
- 列表列：`id, kind, preview, source_app, created_at, last_copied_at, copy_count, pinned, truncated, byte_size, thumb_path`
  —— **不取 `text_body`**（§6.1「列表只返回 preview」）
- `total`：同条件 `COUNT(*)`（不带 limit/offset）
- `limit` 夹取 `1..=200`，`offset >= 0`；超出上限被夹取时打一条 `debug` 日志（只含请求值与夹取后值，不含内容，S11）

### 3. 详情 / 置顶 / 删除

```rust
pub fn get(&self, id: i64) -> Result<ClipboardDetail, AppError>
pub fn set_pinned(&self, id: i64, pinned: bool) -> Result<(), AppError>
pub fn delete(&self, id: i64, hard: bool) -> Result<(), AppError>
```

- `get`：id 不存在 → `AppError::Storage`。**不新增错误码**（不改 §12.1 注册表）：
  前端对 detail 失败统一按「条目已不存在」处理 —— 底栏提示「条目已不存在，列表已刷新」+
  重新拉首屏 + 选中第一项，而不是把 `storage.error` 一律当作数据库故障（S1）。
- `delete`：事务内删 FTS 行 + 主表行；事务外 `hard=true` 删 blob（失败只记日志，不回滚）
- 置顶/删除后前端立即刷新（不依赖事件）

### 4. 前端防抖与竞态

```ts
const requestIdRef = useRef(0);
useEffect(() => {
  const timer = window.setTimeout(() => {
    const id = ++requestIdRef.current;
    setLoading(true);
    ipc.clipboardList({ query, filter, limit: 50, offset: 0 })
      .then((res) => { if (id === requestIdRef.current) { setItems(res.items); setTotal(res.total); } })
      .catch((e) => { if (id === requestIdRef.current) setError(toMessage(e)); })
      .finally(() => { if (id === requestIdRef.current) setLoading(false); });
  }, 80);
  return () => window.clearTimeout(timer);
}, [query, filter]);
```

- 详情查询同样用 requestId 丢弃过期响应
- 80ms 防抖 + 请求在飞行时，`loading` 保留上一版列表（不闪空），仅骨架屏用于首屏

### 5. 键盘（UI-SPEC §5）

| 键 | 行为 |
|---|---|
| ↑ / ↓ | 移动选中（边界停住，不循环） |
| PgUp / PgDn | 移动 10 项 |
| Enter | 粘贴 → **T-205 未实现**：底栏提示「粘贴功能开发中」（D2） |
| Ctrl+Enter | 仅复制 → 底栏提示「仅复制功能开发中」（D2） |
| Ctrl+P | 置顶/取消置顶 |
| Delete | 删除（`hard=false`） |
| Shift+Delete | 永久删除（`hard=true`） |
| Tab / Shift+Tab | 循环/反向循环筛选 |
| Esc | 关闭浮层 |

- 组合期间全部不响应（R5）
- 选中变化 → `scrollIntoView({ block: 'nearest' })`
- **PgUp/PgDn 跨已加载页边界（S9）**：目标索引超出已加载范围时，先触发下一页加载，
  加载完成后再移动选中；期间按键不丢弃（只保留最后一次导航意图），避免「按键没反应」。
- **焦点（S6）**：搜索框 `autoFocus` + 浮层 `overlay://shown` 后显式 `focus()` 双保险；
  挂载后断言 `document.activeElement` 是搜索框（失败则再 focus 一次），防止首字符丢失。
- Enter/Ctrl+Enter 的提示用 `aria-live="polite"` 播报，3s 后恢复默认底栏
- 鼠标悬停不改选中，点击才选中；滚轮只滚动列表

### 6. 右侧预览（PreviewPane）

- 文本：全文（≤2000 字符，超出显示「内容较长，已折叠」）
- 图片：`blob_path` → `convertFileSrc` → 等比缩放最大 240×240，棋盘格背景
  - `.dib` → 「该图片格式暂不支持预览」
  - 无 blob（超限条目）→ 「原图过大，未保存原图」
- 文件：文件名 + 目录 + 是否存在（缺失标红）
- 底部：来源应用、完整时间、大小、标记（已截断 / 原图过大）
- **`ClipboardDetail` 字段清单（S12，前端 `ipc.ts` 手工对齐，字段改名两侧同步改）**：
  `id, kind, preview, text_body?, html?, rtf?, file_list?, file_exists?（与 file_list 对应的存在性，缺失项标红）,
   blob_path?（绝对路径）, thumb_path?, byte_size, created_at, last_copied_at, copy_count, pinned,
   truncated, source_app` —— 可选字段用 `null` 而非缺省，避免 TS 侧 `undefined` 分支散落

### 7. 列表行（ItemRow）

- 类型图标：文本 📄 / 富文本 📝 / 图片 🖼 / 文件 📁
- 首行摘要单行省略
- 右侧：来源应用 · 相对时间 · 置顶 📌 · 复制次数 `×N`（N≥2，N3）
- 置顶与非置顶之间分隔线（N4）
- `thumb_path` 恒 NULL → 类型占位图标，不显示 20×20 缩略图（偏差记 TASKS）

### 8. 相对时间（src/lib/clipboard.ts，纯函数 + 单测）

```
<1 分钟      → 刚刚
<60 分钟     → N 分钟前
<24 小时     → N 小时前
昨天         → 昨天 HH:mm
今年内       → MM-DD HH:mm
跨年         → YYYY-MM-DD
```

### 9. 状态（UI-SPEC §6）

- 加载中：3 行骨架（opacity 40%），不用 spinner
- 空历史：图标 + 「还没有记录」+「复制任意内容后会出现在这里」
- 无搜索结果：「没有找到 “xxx”」
- 错误：底栏变 `--warn` + 中文原因

### 10. 面板打开期间刷新

- 监听 `clipboard://changed`（后端已节流 500ms）→ 以当前 query/filter 重新拉取
- **保持浏览位置（T1）**：刷新时按页重拉，直到覆盖「刷新前已加载的条数」或 `total` 变小，
  不把用户从第 2+ 页打回首屏；仍然选中同一个 id。
- 若该 id 已不在结果中（被 TTL/容量清理或筛选变化）→ 回落到第一项，并底栏短暂提示「列表已更新」
- 刷新期间的翻页请求按 N1 的 generation 规则失效并丢弃，避免两套数据拼接

## 测试策略

### Rust 单测（内存 SQLite）
- 搜索条件：空 / 1 字 / 2 字 / 3 字 / 含引号 / 含 `%` `_` / 纯标点
- 列表排序：置顶项在首；同 `last_copied_at` 时 `id DESC` 稳定；FTS 下置顶集合仍最前（D1）
- 筛选：Text（含 html/rtf）/ Image / Files / Favorites
- 分页：limit/offset 与 total 一致；offset 越界返回空
- 详情：存在 / 不存在
- 置顶切换
- 删除：主表 + FTS 都消失；`hard=true` 删 blob 文件，`hard=false` 保留
- FTS 搜到 3 字以上子串；纯标点查询不报错（回退 LIKE）

### 前端单测（vitest）
- 相对时间格式化（全部档位 + 跨年）
- 筛选枚举循环顺序（Tab / Shift+Tab）
- 键盘动作映射（含 IME 组合期不响应、Enter/Ctrl+Enter 返回占位动作）
- 徽标规则：`copy_count` 1/2/9
- 删除后选中落点（中间/末项/空列表，N2）
- 防抖：连续输入只触发一次请求
- 竞态：旧请求结果不覆盖新结果
- 分页：追加去重、`items.length >= total` 停止；**竞态 generation**：新条件翻页响应先于旧条件首屏返回时不得污染列表（S3）
- 选中落点与 PgDn：跨页边界先加载后移动（S9）
- detail 失败：提示「条目已不存在」+ 刷新列表（S1）
- 组件：空态 / 无结果 / 加载态 / 选中移动 / 置顶后重排

## 手测清单
- Alt+V 打开面板，焦点在搜索框
- 输入中文 3 字能搜到；1 字回退匹配；纯标点不报错
- Tab / Shift+Tab 循环筛选
- ↑↓ / PgUp / PgDn 移动选中，右侧预览同步
- Ctrl+P 置顶，置顶项到顶且有 📌 与 `×N` 徽标，分隔线出现
- Enter / Ctrl+Enter 显示「开发中」提示且不误触发其它动作
- Delete 删除后列表与数据库同步，选中落在下一项；Shift+Delete 同时清掉 blob
- 截图后预览区可见图片（asset 协议）；DIB 条目显示格式不支持
- 1000 条数据下搜索中位数 <50ms（N5 口径）
- 中文输入法组合期按 ↓/Enter 不触发
- Esc 关闭

## 与规格的关系
- **不改** `ARCHITECTURE §6.1`（签名照抄 `clipboard_delete { id, hard }`）
- **不改** `ARCHITECTURE §5.3`（排序按 bm25，D1 已确认）
- **新增配置** `tauri.conf.json` 的 `assetProtocol` + `Cargo.toml` 的 `protocol-asset` feature（B1；UI-SPEC §10 已要求 asset 协议加载 blobs）
- **孤儿 blob（S8）**：`hard=false` 保留的 blob 不再被任何行引用，由 T-206 的孤儿回收（仅删 1h 前创建的）处理；
  写入 TASKS.md 实现备注，可追溯
- 已知偏差：v1 无回收站（D3）；`thumb_path` 恒 NULL；`Enter`/`Ctrl+Enter` 本期占位（D2）
