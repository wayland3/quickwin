# T-205 mini-design（第 2 轮）

> 第 1 轮 GLM-5.3 FAIL：hash 抑制不可靠、时序矛盾、PNG-only。本轮改正。
>
> **第 2 轮评审：GLM-5.3（2026-10-28）PASS（附条件）**，三个阻断项已解决。
> 附加条件：R1 激活入口用 `Mutex` 序列化（并发直接拒绝二次调用），序列号区间用
> wrapping 包含并补交错单测；R2 `build_dib_from_bgra` 明确 alpha 策略（v1 统一 0xFF）与
> bottom-up 行序并测试，手测补 Paint/Word；R3 `suppress.begin` 收窄到紧贴 write 之前、
> hash 兜底口径、降级分支明确「restore 失败仍先写剪贴板」，T-107 需补登记「还原开关持久化」。

## 实现阶段偏差（2026-10-29）

- **hash 口径**：抑制兜底使用 `storage::content_hash`（按类型规范化，文本换行统一、文件路径排序），
  而非写入字节原样 hash。理由：HTML 写回必须重新拼 `HTML Format` 头，读回又是 Fragment，原始字节
  hash 永远不相等；两侧共用同一规范化函数才能自洽。代价是 1.5s 内用户复制「仅换行/文件顺序不同」
  的高度相似内容会被当作回声跳过（不新增、不重排），已接受并记录。
- **还原写序列化**：`schedule_restore` 线程在写回前 `try_lock` 同一把 paste 锁，并走完整
  `begin/seal`（不再只 `add_hash`），避免与下一次激活互相覆盖。
- **写剪贴板部分失败**：不再二次 `EmptyClipboard`，保留已成功写入的格式（如 PNG 已可用）。
- **「目标不可输入」探测**：`send_paste` 返回 Ok 只代表按键注入成功，v1 无法可靠判断目标控件是否
  接受粘贴；该边界按 PRD 6.4 只处理「焦点恢复失败 / 提权」，并列入已知限制（T-703 冒烟清单）。
- **还原开关持久化**：本轮只提供 `clipboard_set_restore_after_paste` 进程内开关，T-107 需把
  「粘贴后还原原剪贴板」纳入设置持久化清单，T-601 提供 UI。

## 改动
- `platform/clipboard.rs`：`write_clipboard_content`，单次 OpenClipboard 会话写条目全部可写格式；纯函数 `build_html_clipboard_payload` / `build_dropfiles` / `build_dib_from_bgra`。
- `platform/imaging.rs`（新）：WIC PNG→BGRA 解码，用于合成 CF_DIB。
- `services/clipboard.rs`：`PasteSuppression`（序列号区间为主 + hash 兜底）；监听线程命中即跳过（不插入/不发事件/不刷新时间）。
- `services/storage.rs`：`compute_hash` → `pub content_hash`（行为不变）。
- `services/window.rs`：`hide_for_paste`（立即隐藏→等待生效→同步 restore）返回 `RestoreOutcome`；`resume_clipboard`（复用 prev_window，不重新 capture）。
- `services/paste.rs`（新）：编排层，T-505 复用。
- `commands/clipboard.rs`：`clipboard_activate(id, mode)`（paste/copy）；`lib.rs` 注册+管理抑制状态。
- `src/lib/ipc.ts` / `ClipboardPanel.tsx` / `App.tsx`：Enter、Ctrl+Enter 接线；`app://toast` 在 App 挂载时已订阅。
- `docs/ARCHITECTURE.md` §6.1/§7.3 落文。

## 顺序（唯一版本，对齐 PRD 6.4）
`read detail → 提权预检 → suppress.begin(seq0) → hide_for_paste → restore 校验 → write_clipboard → suppress.seal(seq1) → send_paste`

suppress.begin 只是打开「写入窗口」（记录起始 seq），写入完成后 seal 精确区间；因此抑制在写剪贴板前已生效，同时对外顺序仍符合 PRD。

## 抑制
`PasteSuppression { ranges: Vec<(u32,u32)>, hashes: Vec<String>, expires, pending_from: Option<u32> }`：
命中任一区间、pending 窗口内 seq 前进、或 hash 相等 → 跳过。1.5s 过期清理。还原原剪贴板时对还原写入同样 begin/seal 并登记快照 hash。

## 线程与窗口
整个 `paste::activate` 在 `spawn_blocking` 内执行（`send_paste`/`restore` 阻塞）。用 `AppHandle` 取 window 与 OverlayManager；`hide_for_paste` 置 Hidden → `window.hide()` → 等 20ms → `restore(…,150)`。不调异步 hide（其 80ms 线程时序不可控）。

## 格式
- 文本：CF_UNICODETEXT。HTML：重新拼 HTML Format 头 + CF_UNICODETEXT 纯文本。RTF：Rich Text Format + 纯文本。
- 图片：PNG blob 写 PNG 注册格式，并用 WIC 解码后补写 CF_DIB；DIB blob 直接写 CF_DIB 与 PNG 跳过。>8MB 无 blob → 提示不可粘贴。
- 文件：CF_HDROP（DROPFILES + 双 NUL）。
- 偏差：T-202 每条约只存 1 种内容（text/html/rtf 互斥），无法还原原剪贴板的全部并行格式；v1 写「存储格式 + 派生纯文本」，不做 schema 变更。

## 降级
- prev=None 或 `is_elevated`：不隐藏、不发键，只写剪贴板 + `app://toast`。
- restore 失败 / send_paste 失败：已写剪贴板 → `resume_clipboard` + `app://toast`「已复制，请手动粘贴」。
- 写剪贴板失败：restore 已发生则 resume + toast「复制失败，请重试」；面板保持。
- 还原开关（默认关，`AtomicBool` + `clipboard_set_restore_after_paste`，持久化随 T-107）：Ctrl+V 后 300ms 写回快照，写回前校验 seq 未再变，变了放弃。UI 文案需说明与「保留在剪贴板」的取舍（T-601）。

## 测试
- Rust：HTML 头偏移（ASCII/中文/空）、DROPFILES 结构、DIB 头字段、抑制（区间/pending/过期/不同 hash，注入 now）、降级判定纯函数、write→read 全格式 round-trip hash 一致、`content_hash` 回归。
- 前端：Enter/Ctrl+Enter 参数、toast 渲染、失败不卸载面板。
- 手测：记事本/Chrome/VS Code/资源管理器；文本/emoji/多行/图片/文件；连续 5 条；管理员窗口；关窗口后降级；还原开关。
