// SQLite 存储层
//
// 管理剪贴板条目的持久化存储，包括：
// - 数据库初始化与迁移（v1）
// - 条目去重入库（hash 唯一索引 + 计数刷新）
// - FTS5 行同步与启动时一致性校验
// - 图片 blob 落盘
//
// 所有写操作都在单事务内完成，失败自动回滚。

use crate::error::AppError;
use crate::platform::clipboard::{ClipboardContent, ClipboardEntry, ImageFormat};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 预览最大字符数（按字符计，避免截断多字节字符）
const PREVIEW_MAX_CHARS: usize = 512;

/// 当前 schema 版本
const SCHEMA_VERSION: i64 = 1;

/// v1 schema
const SCHEMA_V1: &str = r#"
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
    file_list      TEXT,
    source_app     TEXT,
    source_title   TEXT,
    byte_size      INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL,
    last_copied_at INTEGER NOT NULL,
    copy_count     INTEGER NOT NULL DEFAULT 1,
    pinned         INTEGER NOT NULL DEFAULT 0,
    sensitive      INTEGER NOT NULL DEFAULT 0,
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
"#;

/// 入库结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertOutcome {
    /// 新条目，附带 id
    Inserted(i64),
    /// 重复条目（已刷新计数与时间），附带真实 id
    Updated(i64),
}

impl InsertOutcome {
    /// 条目 id（新插入或去重后的真实 id）
    pub fn id(self) -> i64 {
        match self {
            Self::Inserted(id) | Self::Updated(id) => id,
        }
    }

    /// 是否为新条目
    pub fn is_new(self) -> bool {
        matches!(self, Self::Inserted(_))
    }
}

/// 剪贴板存储
pub struct ClipboardStorage {
    conn: Connection,
    /// 数据目录（数据库与 blobs 的父目录）
    data_dir: PathBuf,
}

impl ClipboardStorage {
    /// 创建生产环境存储实例
    ///
    /// 数据库位于 `%APPDATA%\com.quickwin.app\quickwin.db`，blob 位于同目录下 `blobs\`。
    pub fn new() -> Result<Self, AppError> {
        let data_dir = Self::default_data_dir()?;
        Self::open_at(&data_dir.join("quickwin.db"))
    }

    /// 打开指定路径的数据库（供测试与后续迁移使用）
    pub fn open_at(db_path: &Path) -> Result<Self, AppError> {
        let data_dir = db_path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| AppError::Storage("invalid database path".to_string()))?;

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| AppError::Storage(format!("failed to create data directory: {}", e)))?;

        let conn = Connection::open(db_path)
            .map_err(|e| AppError::Storage(format!("failed to open database: {}", e)))?;
        Self::configure(&conn)?;

        let mut storage = Self { conn, data_dir };
        storage.migrate()?;
        storage.check_fts_consistency()?;

        Ok(storage)
    }

    /// 内存数据库（仅测试使用）
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, AppError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| AppError::Storage(format!("failed to open in-memory database: {}", e)))?;
        Self::configure(&conn)?;

        let mut storage = Self {
            conn,
            data_dir: std::env::temp_dir().join("quickwin-test"),
        };
        storage.migrate()?;
        storage.check_fts_consistency()?;

        Ok(storage)
    }

    /// 默认数据目录：`%APPDATA%\com.quickwin.app`
    fn default_data_dir() -> Result<PathBuf, AppError> {
        let app_data = dirs::data_dir()
            .ok_or_else(|| AppError::Storage("failed to get data directory".to_string()))?;
        Ok(app_data.join("com.quickwin.app"))
    }

    /// 设置连接级 PRAGMA
    fn configure(conn: &Connection) -> Result<(), AppError> {
        // journal_mode 会返回一行结果，必须用 query_row 而不是 execute
        conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| AppError::Storage(format!("failed to set journal_mode: {}", e)))?;

        conn.execute_batch(
            "PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;",
        )
        .map_err(|e| AppError::Storage(format!("failed to set pragmas: {}", e)))?;

        Ok(())
    }

    /// 执行迁移（当前仅 v0 → v1）
    fn migrate(&mut self) -> Result<(), AppError> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| AppError::Storage(format!("failed to read user_version: {}", e)))?;

        if version == SCHEMA_VERSION {
            return Ok(());
        }

        if version != 0 {
            // 未来版本：需要按顺序补迁移，禁止跳过
            return Err(AppError::Storage(format!(
                "unsupported schema version: {}",
                version
            )));
        }

        let tx = self
            .conn
            .transaction()
            .map_err(|e| AppError::Storage(format!("failed to begin migration: {}", e)))?;

        tx.execute_batch(SCHEMA_V1)
            .map_err(|e| AppError::Storage(format!("failed to apply schema v1: {}", e)))?;

        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|e| AppError::Storage(format!("failed to set user_version: {}", e)))?;

        tx.commit()
            .map_err(|e| AppError::Storage(format!("failed to commit migration: {}", e)))?;

        log::info!("database migrated: version={}", SCHEMA_VERSION);
        Ok(())
    }

    /// 启动时校验 FTS 与主表行数一致，不一致则重建
    fn check_fts_consistency(&self) -> Result<(), AppError> {
        let main_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM clipboard_item", [], |row| row.get(0))
            .map_err(|e| AppError::Storage(format!("failed to count clipboard_item: {}", e)))?;

        let fts_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM clipboard_fts", [], |row| row.get(0))
            .map_err(|e| AppError::Storage(format!("failed to count clipboard_fts: {}", e)))?;

        if main_count != fts_count {
            log::warn!(
                "FTS inconsistency detected: main={}, fts={}, rebuilding",
                main_count,
                fts_count
            );

            self.conn
                .execute(
                    "INSERT INTO clipboard_fts(clipboard_fts) VALUES('rebuild')",
                    [],
                )
                .map_err(|e| AppError::Storage(format!("failed to rebuild FTS: {}", e)))?;
        }

        Ok(())
    }

    /// 入库
    ///
    /// - 重复内容（hash 相同）只刷新 `copy_count` 与 `last_copied_at`，返回真实 id
    /// - 新内容写入主表 + FTS，图片同时落盘 blob
    /// - 单事务提交，任一步失败自动回滚
    pub fn insert(&mut self, entry: &ClipboardEntry) -> Result<InsertOutcome, AppError> {
        let hash = Self::compute_hash(entry)?;
        let kind = entry.content.kind();
        let preview = build_preview(&entry.preview);
        let now = now_ms()?;
        let byte_size = byte_size(entry);
        let truncated = entry.truncated as i32;

        let tx = self
            .conn
            .transaction()
            .map_err(|e| AppError::Storage(format!("failed to begin transaction: {}", e)))?;

        // 1. 去重：同一 hash 只刷新计数与时间
        let existing_id: Option<i64> = tx
            .query_row(
                "SELECT id FROM clipboard_item WHERE hash = ?1",
                params![hash],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| AppError::Storage(format!("failed to query existing item: {}", e)))?;

        if let Some(id) = existing_id {
            tx.execute(
                "UPDATE clipboard_item SET copy_count = copy_count + 1, last_copied_at = ?1 WHERE id = ?2",
                params![now, id],
            )
            .map_err(|e| AppError::Storage(format!("failed to refresh copy_count: {}", e)))?;

            tx.commit()
                .map_err(|e| AppError::Storage(format!("failed to commit dedupe: {}", e)))?;

            // 内容未变，FTS 无需重写
            log::debug!("clipboard item deduped: id={}, kind={}", id, kind);
            return Ok(InsertOutcome::Updated(id));
        }

        // 2. 图片先落盘（失败则不写库，避免留下无 blob 的图片条目）
        let blob_path = match &entry.content {
            ClipboardContent::Image { data, format } if !data.is_empty() => {
                Some(write_blob(&self.data_dir, &hash, data, *format)?)
            }
            // 超过 8MB 的图片只保留标记条目，不落盘
            _ => None,
        };

        let (text_body, html, rtf, file_list) = match &entry.content {
            ClipboardContent::Text(text) => (Some(text.as_str()), None, None, None),
            ClipboardContent::Html(body) => (None, Some(body.as_str()), None, None),
            ClipboardContent::Rtf(body) => (None, None, Some(body.as_str()), None),
            ClipboardContent::Image { .. } => (None, None, None, None),
            ClipboardContent::Files(files) => {
                let json = serde_json::to_string(files).map_err(|e| {
                    AppError::Storage(format!("failed to serialize file list: {}", e))
                })?;
                (None, None, None, Some(json))
            }
        };

        // 3. 写主表
        tx.execute(
            "INSERT INTO clipboard_item (
                kind, hash, preview, text_body, html, rtf, blob_path, thumb_path,
                file_list, source_app, source_title, byte_size, created_at, last_copied_at,
                copy_count, pinned, sensitive, truncated
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9, ?10, ?11, ?12, ?12, 1, 0, 0, ?13)",
            params![
                kind,
                hash,
                preview,
                text_body,
                html,
                rtf,
                blob_path,
                file_list,
                entry.source_app,
                entry.source_title,
                byte_size,
                now,
                truncated
            ],
        )
        .map_err(|e| AppError::Storage(format!("failed to insert item: {}", e)))?;

        let id = tx.last_insert_rowid();

        // 4. 同步 FTS（显式 rowid = clipboard_item.id）
        tx.execute(
            "INSERT INTO clipboard_fts (rowid, preview, text_body) VALUES (?1, ?2, ?3)",
            params![id, preview, text_body],
        )
        .map_err(|e| AppError::Storage(format!("failed to sync FTS: {}", e)))?;

        tx.commit()
            .map_err(|e| AppError::Storage(format!("failed to commit insert: {}", e)))?;

        log::debug!(
            "clipboard item inserted: id={}, kind={}, bytes={}",
            id,
            kind,
            byte_size
        );
        Ok(InsertOutcome::Inserted(id))
    }

    /// 计算去重 hash：`sha256(kind + 规范化内容)`
    ///
    /// 规范化：
    /// - 文本类：换行统一为 `\n`
    /// - 文件列表：按路径排序后序列化，复制顺序不影响 hash
    fn compute_hash(entry: &ClipboardEntry) -> Result<String, AppError> {
        let mut hasher = Sha256::new();

        match &entry.content {
            ClipboardContent::Text(text) => {
                hasher.update(b"text:");
                hasher.update(normalize_newlines(text).as_bytes());
            }
            ClipboardContent::Html(html) => {
                hasher.update(b"html:");
                hasher.update(normalize_newlines(html).as_bytes());
            }
            ClipboardContent::Rtf(rtf) => {
                hasher.update(b"rtf:");
                hasher.update(normalize_newlines(rtf).as_bytes());
            }
            ClipboardContent::Image { data, format } => {
                hasher.update(b"image:");
                hasher.update(format.extension().as_bytes());
                hasher.update(b":");
                hasher.update(data);
            }
            ClipboardContent::Files(files) => {
                hasher.update(b"files:");
                let mut sorted = files.clone();
                sorted.sort();
                let json = serde_json::to_string(&sorted)
                    .map_err(|e| AppError::Storage(format!("failed to serialize files: {}", e)))?;
                hasher.update(json.as_bytes());
            }
        }

        Ok(format!("{:x}", hasher.finalize()))
    }
}

/// 写图片 blob：`blobs\<hash 前两位>\<hash>.<ext>`，返回相对数据目录的路径
fn write_blob(
    data_dir: &Path,
    hash: &str,
    bytes: &[u8],
    format: ImageFormat,
) -> Result<String, AppError> {
    let shard = hash.get(0..2).unwrap_or("00");
    let relative = format!("blobs/{}/{}.{}", shard, hash, format.extension());
    let absolute = data_dir.join(&relative);

    if let Some(parent) = absolute.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Storage(format!("failed to create blob directory: {}", e)))?;
    }

    if !absolute.exists() {
        std::fs::write(&absolute, bytes)
            .map_err(|e| AppError::Storage(format!("failed to write blob: {}", e)))?;
    }

    Ok(relative)
}

/// 当前时间（unix 毫秒）
fn now_ms() -> Result<i64, AppError> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| AppError::Internal(format!("failed to read system time: {}", e)))?
        .as_millis() as i64)
}

/// 统一换行符为 `\n`
fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// 生成预览：去除控制字符并限制为 512 字符
fn build_preview(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect();

    cleaned.chars().take(PREVIEW_MAX_CHARS).collect()
}

/// 条目原始字节数
fn byte_size(entry: &ClipboardEntry) -> i64 {
    match &entry.content {
        ClipboardContent::Text(text) => text.len() as i64,
        ClipboardContent::Html(html) => html.len() as i64,
        ClipboardContent::Rtf(rtf) => rtf.len() as i64,
        ClipboardContent::Image { data, .. } => data.len() as i64,
        ClipboardContent::Files(files) => serde_json::to_string(files)
            .map(|s| s.len() as i64)
            .unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_entry(text: &str) -> ClipboardEntry {
        ClipboardEntry {
            content: ClipboardContent::Text(text.to_string()),
            preview: text.to_string(),
            truncated: false,
            source_app: Some("notepad.exe".to_string()),
            source_title: None,
        }
    }

    fn open_test_storage() -> ClipboardStorage {
        match ClipboardStorage::open_in_memory() {
            Ok(storage) => storage,
            Err(e) => panic!("failed to open test storage: {}", e),
        }
    }

    fn count_items(storage: &ClipboardStorage) -> i64 {
        match storage
            .conn
            .query_row("SELECT COUNT(*) FROM clipboard_item", [], |row| row.get(0))
        {
            Ok(count) => count,
            Err(e) => panic!("failed to count items: {}", e),
        }
    }

    fn copy_count(storage: &ClipboardStorage, id: i64) -> i64 {
        match storage.conn.query_row(
            "SELECT copy_count FROM clipboard_item WHERE id = ?1",
            params![id],
            |row| row.get(0),
        ) {
            Ok(count) => count,
            Err(e) => panic!("failed to read copy_count: {}", e),
        }
    }

    fn search_fts(storage: &ClipboardStorage, query: &str) -> i64 {
        match storage.conn.query_row(
            "SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH ?1",
            params![query],
            |row| row.get(0),
        ) {
            Ok(count) => count,
            Err(e) => panic!("failed to query FTS: {}", e),
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 同一内容复制三次只有一条记录且计数为三() {
        let mut storage = open_test_storage();
        let entry = text_entry("QuickWin 去重测试");

        let first = match storage.insert(&entry) {
            Ok(outcome) => outcome,
            Err(e) => panic!("first insert failed: {}", e),
        };
        assert!(first.is_new());

        for _ in 0..2 {
            match storage.insert(&entry) {
                Ok(outcome) => {
                    assert!(!outcome.is_new());
                    assert_eq!(outcome.id(), first.id());
                }
                Err(e) => panic!("repeat insert failed: {}", e),
            }
        }

        assert_eq!(count_items(&storage), 1);
        assert_eq!(copy_count(&storage, first.id()), 3);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 预览不含控制字符() {
        let mut storage = open_test_storage();
        let entry = ClipboardEntry {
            content: ClipboardContent::Text("Hello\x00World\x07Test".to_string()),
            preview: "Hello\x00World\x07Test".to_string(),
            truncated: false,
            source_app: None,
            source_title: None,
        };

        let outcome = match storage.insert(&entry) {
            Ok(outcome) => outcome,
            Err(e) => panic!("insert failed: {}", e),
        };

        let preview: String = match storage.conn.query_row(
            "SELECT preview FROM clipboard_item WHERE id = ?1",
            params![outcome.id()],
            |row| row.get(0),
        ) {
            Ok(preview) => preview,
            Err(e) => panic!("failed to read preview: {}", e),
        };

        assert_eq!(preview, "HelloWorldTest");
        assert!(!preview.chars().any(|c| c.is_control()));
    }

    #[test]
    #[allow(non_snake_case)]
    fn 预览最多五百一十二字符() {
        let mut storage = open_test_storage();
        let long = "长".repeat(600);
        let entry = text_entry(&long);

        let outcome = match storage.insert(&entry) {
            Ok(outcome) => outcome,
            Err(e) => panic!("insert failed: {}", e),
        };

        let preview: String = match storage.conn.query_row(
            "SELECT preview FROM clipboard_item WHERE id = ?1",
            params![outcome.id()],
            |row| row.get(0),
        ) {
            Ok(preview) => preview,
            Err(e) => panic!("failed to read preview: {}", e),
        };

        assert_eq!(preview.chars().count(), PREVIEW_MAX_CHARS);
    }

    #[test]
    #[allow(non_snake_case)]
    fn FTS能搜到三字以上子串() {
        let mut storage = open_test_storage();
        let entry = text_entry("QuickWin 剪贴板历史测试");

        match storage.insert(&entry) {
            Ok(_) => {}
            Err(e) => panic!("insert failed: {}", e),
        }

        // trigram 分词：≥3 字可命中子串
        assert_eq!(search_fts(&storage, "\"剪贴板历\""), 1);
        assert_eq!(search_fts(&storage, "\"QuickWin\""), 1);
    }

    #[test]
    #[allow(non_snake_case)]
    fn FTS行与主表行数一致() {
        let mut storage = open_test_storage();
        for text in ["第一条内容", "第二条内容", "第三条内容"] {
            match storage.insert(&text_entry(text)) {
                Ok(_) => {}
                Err(e) => panic!("insert failed: {}", e),
            }
        }

        assert_eq!(count_items(&storage), 3);
        assert_eq!(search_fts(&storage, "\"条内容\""), 3);
    }

    #[test]
    #[allow(non_snake_case)]
    fn hash忽略换行差异与文件顺序() {
        let lf = ClipboardEntry {
            content: ClipboardContent::Text("a\nb".to_string()),
            preview: "a\nb".to_string(),
            truncated: false,
            source_app: None,
            source_title: None,
        };
        let crlf = ClipboardEntry {
            content: ClipboardContent::Text("a\r\nb".to_string()),
            preview: "a\r\nb".to_string(),
            truncated: false,
            source_app: None,
            source_title: None,
        };
        assert_eq!(
            ClipboardStorage::compute_hash(&lf).ok(),
            ClipboardStorage::compute_hash(&crlf).ok()
        );

        let files_a = ClipboardEntry {
            content: ClipboardContent::Files(vec!["C:/b.txt".to_string(), "C:/a.txt".to_string()]),
            preview: "[2 个文件]".to_string(),
            truncated: false,
            source_app: None,
            source_title: None,
        };
        let files_b = ClipboardEntry {
            content: ClipboardContent::Files(vec!["C:/a.txt".to_string(), "C:/b.txt".to_string()]),
            preview: "[2 个文件]".to_string(),
            truncated: false,
            source_app: None,
            source_title: None,
        };
        assert_eq!(
            ClipboardStorage::compute_hash(&files_a).ok(),
            ClipboardStorage::compute_hash(&files_b).ok()
        );
    }

    #[test]
    #[allow(non_snake_case)]
    fn 文件列表以JSON入库() {
        let mut storage = open_test_storage();
        let entry = ClipboardEntry {
            content: ClipboardContent::Files(vec![
                "C:/tmp/a.txt".to_string(),
                "C:/tmp/b.txt".to_string(),
            ]),
            preview: "[2 个文件]".to_string(),
            truncated: false,
            source_app: None,
            source_title: None,
        };

        let outcome = match storage.insert(&entry) {
            Ok(outcome) => outcome,
            Err(e) => panic!("insert failed: {}", e),
        };

        let file_list: String = match storage.conn.query_row(
            "SELECT file_list FROM clipboard_item WHERE id = ?1",
            params![outcome.id()],
            |row| row.get(0),
        ) {
            Ok(list) => list,
            Err(e) => panic!("failed to read file_list: {}", e),
        };

        let parsed: Vec<String> = match serde_json::from_str(&file_list) {
            Ok(parsed) => parsed,
            Err(e) => panic!("file_list is not valid JSON: {}", e),
        };
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 图片写入blob并记录路径() {
        let unique = format!(
            "quickwin-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let db_path = std::env::temp_dir().join(unique).join("quickwin.db");

        let mut storage = match ClipboardStorage::open_at(&db_path) {
            Ok(storage) => storage,
            Err(e) => panic!("failed to open storage: {}", e),
        };

        let entry = ClipboardEntry {
            content: ClipboardContent::Image {
                data: vec![0x89, 0x50, 0x4E, 0x47],
                format: ImageFormat::Png,
            },
            preview: "[图片]".to_string(),
            truncated: false,
            source_app: None,
            source_title: None,
        };

        let outcome = match storage.insert(&entry) {
            Ok(outcome) => outcome,
            Err(e) => panic!("insert failed: {}", e),
        };

        let blob_path: String = match storage.conn.query_row(
            "SELECT blob_path FROM clipboard_item WHERE id = ?1",
            params![outcome.id()],
            |row| row.get(0),
        ) {
            Ok(path) => path,
            Err(e) => panic!("failed to read blob_path: {}", e),
        };

        let absolute = storage.data_dir.join(&blob_path);
        assert!(absolute.exists(), "blob file should exist");

        // 清理测试目录
        if let Some(dir) = db_path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 超大图片只记录标记条目() {
        let mut storage = open_test_storage();
        let entry = ClipboardEntry {
            content: ClipboardContent::Image {
                data: Vec::new(),
                format: ImageFormat::Dib,
            },
            preview: "[图片过大]".to_string(),
            truncated: true,
            source_app: None,
            source_title: None,
        };

        let outcome = match storage.insert(&entry) {
            Ok(outcome) => outcome,
            Err(e) => panic!("insert failed: {}", e),
        };

        let (blob_path, truncated): (Option<String>, i32) = match storage.conn.query_row(
            "SELECT blob_path, truncated FROM clipboard_item WHERE id = ?1",
            params![outcome.id()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ) {
            Ok(values) => values,
            Err(e) => panic!("failed to read row: {}", e),
        };

        assert!(blob_path.is_none(), "超限图片不应写 blob");
        assert_eq!(truncated, 1);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 迁移幂等且版本为一() {
        let mut storage = open_test_storage();
        let version: i64 = match storage
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
        {
            Ok(version) => version,
            Err(e) => panic!("failed to read user_version: {}", e),
        };
        assert_eq!(version, SCHEMA_VERSION);

        // 重复执行不应报错或改版本
        match storage.migrate() {
            Ok(()) => {}
            Err(e) => panic!("second migration failed: {}", e),
        }

        let version_again: i64 = match storage
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
        {
            Ok(version) => version,
            Err(e) => panic!("failed to read user_version: {}", e),
        };
        assert_eq!(version_again, SCHEMA_VERSION);
    }
}
