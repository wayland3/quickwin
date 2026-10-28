# T-203 mini-design v4

## 改动文件
- `src-tauri/src/services/storage.rs`（新增）- SQLite 存储层
- `src-tauri/src/services/clipboard.rs`（修改）- 入库管线
- `src-tauri/src/models.rs`（修改）- 添加数据库相关结构
- `src-tauri/Cargo.toml`（修改）- 添加 rusqlite、sha2 依赖
- `docs/ARCHITECTURE.md`（修改）- 更新事件表 §6.2

## 模块交互
```
services::clipboard::ClipboardMonitor
  → services::storage::ClipboardStorage::insert
    → 计算 hash（文本：sha256(kind + text)；图片：sha256(kind + bytes)；文件：sha256(kind + json)）
    → 开始事务 BEGIN
    → 尝试 INSERT（UNIQUE 约束）
    → 如果冲突：查询原有 id，UPDATE copy_count+1, last_copied_at=now
    → 更新 FTS（INSERT INTO clipboard_fts(rowid, preview, text_body) VALUES (?, ?, ?)）
      rowid = clipboard_item.id（显式指定，保证与主表关联）
    → COMMIT（任一失败则 ROLLBACK）
  → 成功（新条目）：emit clipboard://changed（载荷 { id, kind }，节流 500ms）
  → 重复（copy_count 更新）：emit clipboard://changed（载荷 { id, kind }，节流 500ms）
  → 失败：emit clipboard://insert-error（载荷 { code, message }，不节流，立即通知）
    → 前端监听 clipboard://insert-error，映射到 app://toast 显示中文提示
```

## 关键实现点

### 1. 数据库初始化与迁移
```rust
pub struct ClipboardStorage {
    conn: rusqlite::Connection,
}

impl ClipboardStorage {
    pub fn new() -> Result<Self, AppError> {
        // 打开数据库（%APPDATA%/com.quickwin.app/quickwin.db）
        // 设置 PRAGMA：WAL、NORMAL、busy_timeout=5000、foreign_keys=ON
        // 检查 user_version，执行迁移（v0 → v1）
        // 启动时一致性检查：对比主表与 FTS 表行数，差异时重建 FTS
    }
    
    fn migrate(&self) -> Result<(), AppError> {
        // 迁移 v1：创建 clipboard_item、clipboard_fts、索引
        // 更新 user_version = 1
    }
    
    fn check_fts_consistency(&self) -> Result<(), AppError> {
        // 对比 clipboard_item 和 clipboard_fts 行数
        // 差异时执行：INSERT INTO clipboard_fts(clipboard_fts) VALUES('rebuild')
    }
}
```

### 2. 入库逻辑（事务包裹，FTS 显式 rowid，UNIQUE 冲突返回真实 id）
```rust
pub fn insert(&self, entry: ClipboardEntry) -> Result<i64, AppError> {
    // 1. 计算 hash
    let hash = match &entry.content {
        ClipboardContent::Text(text) => sha256(format!("text:{}", text)),
        ClipboardContent::Html(html) => sha256(format!("html:{}", html)),
        ClipboardContent::Rtf(rtf) => sha256(format!("rtf:{}", rtf)),
        ClipboardContent::Image(bytes) => sha256_bytes(b"image:", bytes),
        ClipboardContent::Files(files) => sha256(format!("files:{}", serde_json::to_string(files)?)),
    };
    
    // 2. 开始事务
    self.conn.execute("BEGIN", [])?;
    
    // 3. 尝试 INSERT
    let result = self.conn.execute(
        "INSERT INTO clipboard_item (kind, hash, preview, text_body, html, rtf, blob_path, thumb_path, file_list, source_app, source_title, byte_size, created_at, last_copied_at, copy_count, pinned, sensitive, truncated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 1, 0, 0, ?15)",
        rusqlite::params![kind, hash, preview, text_body, html, rtf, blob_path, thumb_path, file_list, source_app, source_title, byte_size, now, now, truncated],
    );
    
    match result {
        Ok(_) => {
            // 新条目：获取 rowid，更新 FTS（显式指定 rowid）
            let rowid = self.conn.last_insert_rowid();
            self.conn.execute(
                "INSERT INTO clipboard_fts (rowid, preview, text_body) VALUES (?1, ?2, ?3)",
                rusqlite::params![rowid, preview, text_body],
            )?;
            self.conn.execute("COMMIT", [])?;
            Ok(rowid)
        }
        Err(rusqlite::Error::SqliteFailure(err, _)) if err.extended_code == 2067 => {
            // UNIQUE 约束冲突：查询原有 id，更新 copy_count 和 last_copied_at
            let existing_id: i64 = self.conn.query_row(
                "SELECT id FROM clipboard_item WHERE hash = ?1",
                rusqlite::params![hash],
                |row| row.get(0),
            )?;
            self.conn.execute(
                "UPDATE clipboard_item SET copy_count = copy_count + 1, last_copied_at = ?1 WHERE id = ?2",
                rusqlite::params![now, existing_id],
            )?;
            self.conn.execute("COMMIT", [])?;
            Ok(existing_id) // 返回真实 id，前端可定位
        }
        Err(e) => {
            self.conn.execute("ROLLBACK", [])?;
            Err(AppError::Storage(e.to_string()))
        }
    }
}
```

### 3. 事件节流（无 unwrap）
```rust
struct ThrottledEmitter {
    last_emit: Arc<Mutex<Instant>>,
}

impl ThrottledEmitter {
    pub fn emit(&self, app: &AppHandle, event: &str, data: serde_json::Value) -> Result<(), AppError> {
        let now = Instant::now();
        let mut last = self.last_emit.lock().map_err(|e| AppError::Internal(format!("lock poisoned: {}", e)))?;
        if now.duration_since(*last) >= Duration::from_millis(500) {
            app.emit(event, data).map_err(|e| AppError::Internal(e.to_string()))?;
            *last = now;
        }
        Ok(())
    }
}
```

### 4. 错误事件传播（完整链路）
```rust
// 成功/重复：emit clipboard://changed（节流 500ms）
// 载荷：{ id: i64, kind: String }
if let Err(e) = emitter.emit(&app, "clipboard://changed", json!({ "id": rowid, "kind": kind })) {
    log::warn!("emit clipboard://changed failed: {}", e);
}

// 失败：emit clipboard://insert-error（不节流，立即通知）
// 载荷：{ code: String, message: String }
// code 复用 ARCHITECTURE.md §12.1 错误码注册表（如 "storage.error"）
let error_payload = match &e {
    AppError::Storage(_) => json!({
        "code": "storage.error",
        "message": "数据库操作失败"
    }),
    _ => json!({
        "code": "internal",
        "message": "发生内部错误，请查看日志"
    }),
};
let _ = app.emit("clipboard://insert-error", error_payload);
log::error!("clipboard insert failed: code={}, message={}", error_payload["code"], error_payload["message"]);

// 前端监听 clipboard://insert-error，映射到 app://toast
// src/App.tsx:
// listen("clipboard://insert-error", (event) => {
//   const { code, message } = event.payload;
//   toast.error(message); // 显示中文 toast
// });
```

### 5. 预览清理
```rust
fn clean_preview(text: &str) -> String {
    // 移除控制字符（保留换行、制表符）
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}
```

## 测试策略

### 单测
- hash 计算正确性（文本、图片、文件列表）
- 重复检测（copy_count 递增，返回真实 id）
- 预览清理（控制字符移除）
- FTS 同步（主表 INSERT 后 FTS 可搜索，rowid 一致）
- FTS 一致性检查（行数差异时重建）
- 事务回滚（INSERT 失败时 ROLLBACK）
- 高并发写入不崩溃（100 次/秒持续 10 秒）

### 手测入口
创建 `tests/storage_manual.rs`（`#[ignore]`）

### 手测清单
- 同一内容复制 3 次，验证只有 1 条记录且计数为 3
- 预览不含控制字符
- FTS 能搜到 3 字以上子串
- 数据库写入失败时 UI 有提示且不崩（模拟数据库锁定）

## 次要修复
- Cargo.toml 添加 rusqlite（bundled）、sha2 依赖
- 日志合规性：仅记录 id、类型、长度，不记录内容
- 更新 ARCHITECTURE.md §6.2 事件表，新增 clipboard://insert-error
