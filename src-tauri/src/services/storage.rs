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
use crate::models::{ClipFilter, ClipKind, ClipboardDetail, ClipboardItem};
use crate::platform::clipboard::{ClipboardContent, ClipboardEntry, ImageFormat};
use crate::services::query::{self, SearchKind};
use rusqlite::types::FromSql;
use rusqlite::{params, Connection, OptionalExtension, Row};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 列表查询列（不取 `text_body`，遵守 §6.1「列表只返回 preview」）
const LIST_COLUMNS: &str = "id, kind, preview, source_app, created_at, last_copied_at, \
                            copy_count, pinned, sensitive, truncated, byte_size, thumb_path";

/// FTS 联表查询列（需带 `c.` 前缀）
const LIST_COLUMNS_FTS: &str = "c.id, c.kind, c.preview, c.source_app, c.created_at, \
                                c.last_copied_at, c.copy_count, c.pinned, c.sensitive, \
                                c.truncated, c.byte_size, c.thumb_path";

/// 详情查询列
const DETAIL_COLUMNS: &str = "id, kind, preview, text_body, html, rtf, file_list, blob_path, \
                              thumb_path, byte_size, created_at, last_copied_at, copy_count, \
                              pinned, truncated, source_app";

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

/// 数据库路径（只读配置，供 command 层按需开短连接）
#[derive(Debug, Clone)]
pub struct DbPath(PathBuf);

impl DbPath {
    /// 解析默认路径：`%APPDATA%\com.quickwin.app\quickwin.db`
    pub fn resolve() -> Result<Self, AppError> {
        Ok(Self(
            ClipboardStorage::default_data_dir()?.join("quickwin.db"),
        ))
    }

    /// 由外部路径构造（测试与将来迁移用）
    pub fn from_path(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn as_path(&self) -> &Path {
        &self.0
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
        let hash = content_hash(&entry.content)?;
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

    /// 列表查询：返回当前页条目与同条件总数
    ///
    /// - 列表只返回 `preview`，不返回正文（`ARCHITECTURE §6.1`）
    /// - 排序：`pinned DESC` 恒为第一键；FTS 路径集合内按 `bm25` 相关度，末尾 `id DESC` 稳定 tie-break
    pub fn list(
        &self,
        query_text: &str,
        filter: ClipFilter,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<ClipboardItem>, i64), AppError> {
        let (limit, clamped) = query::clamp_limit(limit);
        if clamped {
            log::debug!("clipboard list limit clamped: applied={}", limit);
        }

        let spec = query::build_search(query_text);
        let filter_sql = query::filter_clause(filter);
        let filter_fts = query::filter_clause_fts(filter);

        let like_clause = " AND (preview LIKE ?1 ESCAPE '\\' OR text_body LIKE ?1 ESCAPE '\\')";

        let (rows_sql, count_sql, pattern): (String, String, Option<&str>) = match spec.kind {
            SearchKind::None => (
                format!(
                    "SELECT {} FROM clipboard_item WHERE 1=1{} \
                     ORDER BY pinned DESC, last_copied_at DESC, id DESC LIMIT ?1 OFFSET ?2",
                    LIST_COLUMNS, filter_sql
                ),
                format!(
                    "SELECT COUNT(*) FROM clipboard_item WHERE 1=1{}",
                    filter_sql
                ),
                None,
            ),
            SearchKind::Like => (
                format!(
                    "SELECT {} FROM clipboard_item WHERE 1=1{}{} \
                     ORDER BY pinned DESC, last_copied_at DESC, id DESC LIMIT ?2 OFFSET ?3",
                    LIST_COLUMNS, filter_sql, like_clause
                ),
                format!(
                    "SELECT COUNT(*) FROM clipboard_item WHERE 1=1{}{}",
                    filter_sql, like_clause
                ),
                Some(spec.param.as_str()),
            ),
            SearchKind::Fts => (
                format!(
                    "SELECT {} FROM clipboard_fts f JOIN clipboard_item c ON c.id = f.rowid \
                     WHERE clipboard_fts MATCH ?1{} \
                     ORDER BY c.pinned DESC, bm25(clipboard_fts), c.last_copied_at DESC, c.id DESC \
                     LIMIT ?2 OFFSET ?3",
                    LIST_COLUMNS_FTS, filter_fts
                ),
                format!(
                    "SELECT COUNT(*) FROM clipboard_fts f JOIN clipboard_item c ON c.id = f.rowid \
                     WHERE clipboard_fts MATCH ?1{}",
                    filter_fts
                ),
                Some(spec.param.as_str()),
            ),
        };

        let total: i64 = match pattern {
            Some(p) => self
                .conn
                .query_row(&count_sql, params![p], |row| row.get(0)),
            None => self.conn.query_row(&count_sql, [], |row| row.get(0)),
        }
        .map_err(|e| AppError::Storage(format!("failed to count clipboard items: {}", e)))?;

        let mut stmt = self
            .conn
            .prepare(&rows_sql)
            .map_err(|e| AppError::Storage(format!("failed to prepare list query: {}", e)))?;

        let mut rows = match pattern {
            Some(p) => stmt.query(params![p, i64::from(limit), i64::from(offset)]),
            None => stmt.query(params![i64::from(limit), i64::from(offset)]),
        }
        .map_err(|e| AppError::Storage(format!("failed to run list query: {}", e)))?;

        let mut items = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|e| AppError::Storage(format!("failed to read list row: {}", e)))?
        {
            items.push(map_list_item(row)?);
        }

        Ok((items, total))
    }

    /// 详情查询；id 不存在返回 `None`（由 command 层转成用户可见提示）
    pub fn get(&self, id: i64) -> Result<Option<ClipboardDetail>, AppError> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {} FROM clipboard_item WHERE id = ?1",
                DETAIL_COLUMNS
            ))
            .map_err(|e| AppError::Storage(format!("failed to prepare detail query: {}", e)))?;

        let mut rows = stmt
            .query(params![id])
            .map_err(|e| AppError::Storage(format!("failed to run detail query: {}", e)))?;

        match rows
            .next()
            .map_err(|e| AppError::Storage(format!("failed to read detail row: {}", e)))?
        {
            Some(row) => Ok(Some(map_detail(row, &self.data_dir)?)),
            None => Ok(None),
        }
    }

    /// 置顶 / 取消置顶
    pub fn set_pinned(&self, id: i64, pinned: bool) -> Result<(), AppError> {
        let affected = self
            .conn
            .execute(
                "UPDATE clipboard_item SET pinned = ?1 WHERE id = ?2",
                params![i64::from(pinned), id],
            )
            .map_err(|e| AppError::Storage(format!("failed to update pinned: {}", e)))?;

        if affected == 0 {
            return Err(AppError::Storage(format!(
                "clipboard item not found: {}",
                id
            )));
        }

        log::debug!("clipboard item pin updated: id={}, pinned={}", id, pinned);
        Ok(())
    }

    /// 删除（`hard=true` 时同时删除 blob 文件）
    ///
    /// v1 没有回收站：两个入口都是物理删除，`hard=false` 只保留 blob 文件（孤儿由 T-206 回收）。
    pub fn delete(&mut self, id: i64, hard: bool) -> Result<(), AppError> {
        let blob_relative: Option<Option<String>> = self
            .conn
            .query_row(
                "SELECT blob_path FROM clipboard_item WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| AppError::Storage(format!("failed to query blob_path: {}", e)))?;

        let Some(blob_relative) = blob_relative else {
            return Err(AppError::Storage(format!(
                "clipboard item not found: {}",
                id
            )));
        };

        let tx = self
            .conn
            .transaction()
            .map_err(|e| AppError::Storage(format!("failed to begin delete: {}", e)))?;

        tx.execute("DELETE FROM clipboard_fts WHERE rowid = ?1", params![id])
            .map_err(|e| AppError::Storage(format!("failed to delete FTS row: {}", e)))?;

        tx.execute("DELETE FROM clipboard_item WHERE id = ?1", params![id])
            .map_err(|e| AppError::Storage(format!("failed to delete item: {}", e)))?;

        tx.commit()
            .map_err(|e| AppError::Storage(format!("failed to commit delete: {}", e)))?;

        if hard {
            if let Some(relative) = blob_relative {
                let absolute = self.data_dir.join(relative);
                if let Err(e) = std::fs::remove_file(&absolute) {
                    // blob 已不在不影响删除结果，只记录诊断
                    log::warn!("failed to remove blob for id={}: {}", id, e);
                }
            }
        }

        log::debug!("clipboard item deleted: id={}, hard={}", id, hard);
        Ok(())
    }

    /// 只读路径专用 open：跳过 FTS 一致性全表计数（S5）
    ///
    /// - `user_version == 0`（库从未迁移，例如监听线程初始化失败）→ 先迁移再读，保证空库是空态而非报错
    ///   该分支只在「库从未迁移」的冷启动窗口内写一次；正常运行时库已是当前版本，读路径不产生任何写
    /// - `user_version == SCHEMA_VERSION` → 不执行任何写操作
    /// - 其余版本 → `storage.error`
    pub fn open_readonly_at(db_path: &Path) -> Result<Self, AppError> {
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
        let version = storage.read_user_version()?;

        if version == 0 {
            // 并发首读时另一个连接可能已抢先完成迁移：失败后再读一次版本，已是当前版本则放行
            if let Err(err) = storage.migrate() {
                if storage.read_user_version()? != SCHEMA_VERSION {
                    return Err(err);
                }
            }
        } else if version != SCHEMA_VERSION {
            return Err(AppError::Storage(format!(
                "unsupported schema version: {}",
                version
            )));
        }

        Ok(storage)
    }

    fn read_user_version(&self) -> Result<i64, AppError> {
        self.conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| AppError::Storage(format!("failed to read user_version: {}", e)))
    }
}

/// 计算内容 hash：`sha256(kind + 规范化内容)`
///
/// 规范化：文本类换行统一为 `\n`；文件列表按路径排序后序列化。
/// 入库去重与粘贴抑制共用，保证两边口径一致。
pub fn content_hash(content: &ClipboardContent) -> Result<String, AppError> {
    let mut hasher = Sha256::new();

    match content {
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

/// 读一列并按 `AppError` 包装错误
fn column<T: FromSql>(row: &Row<'_>, idx: usize) -> Result<T, AppError> {
    row.get(idx)
        .map_err(|e| AppError::Storage(format!("failed to read column {}: {}", idx, e)))
}

/// DB 中的 kind 文本 → `ClipKind`
fn kind_from_db(text: &str) -> Result<ClipKind, AppError> {
    match text {
        "text" => Ok(ClipKind::Text),
        "html" => Ok(ClipKind::Html),
        "rtf" => Ok(ClipKind::Rtf),
        "image" => Ok(ClipKind::Image),
        "files" => Ok(ClipKind::Files),
        other => Err(AppError::Storage(format!("unknown clip kind: {}", other))),
    }
}

/// 列表行 → `ClipboardItem`
fn map_list_item(row: &Row<'_>) -> Result<ClipboardItem, AppError> {
    let kind_text: String = column(row, 1)?;
    Ok(ClipboardItem {
        id: column(row, 0)?,
        kind: kind_from_db(&kind_text)?,
        preview: column(row, 2)?,
        source_app: column(row, 3)?,
        created_at: column(row, 4)?,
        last_copied_at: column(row, 5)?,
        copy_count: column(row, 6)?,
        pinned: column::<i64>(row, 7)? != 0,
        sensitive: column::<i64>(row, 8)? != 0,
        truncated: column::<i64>(row, 9)? != 0,
        byte_size: column(row, 10)?,
        thumb_path: column(row, 11)?,
    })
}

/// 详情行 → `ClipboardDetail`
fn map_detail(row: &Row<'_>, data_dir: &Path) -> Result<ClipboardDetail, AppError> {
    let kind_text: String = column(row, 1)?;
    let file_list_json: Option<String> = column(row, 6)?;
    let blob_relative: Option<String> = column(row, 7)?;

    // 文件列表解析失败不阻断详情展示，降级为空
    let file_list = match file_list_json {
        Some(json) => match serde_json::from_str::<Vec<String>>(&json) {
            Ok(files) => Some(files),
            Err(e) => {
                log::warn!("failed to parse file_list: {}", e);
                None
            }
        },
        None => None,
    };
    let file_exists = file_list
        .as_ref()
        .map(|files| files.iter().map(|p| Path::new(p).exists()).collect());

    Ok(ClipboardDetail {
        id: column(row, 0)?,
        kind: kind_from_db(&kind_text)?,
        preview: column(row, 2)?,
        text_body: column(row, 3)?,
        html: column(row, 4)?,
        rtf: column(row, 5)?,
        file_list,
        file_exists,
        blob_path: blob_relative.map(|rel| data_dir.join(rel).to_string_lossy().into_owned()),
        thumb_path: column(row, 8)?,
        byte_size: column(row, 9)?,
        created_at: column(row, 10)?,
        last_copied_at: column(row, 11)?,
        copy_count: column(row, 12)?,
        pinned: column::<i64>(row, 13)? != 0,
        truncated: column::<i64>(row, 14)? != 0,
        source_app: column(row, 15)?,
    })
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
            content_hash(&lf.content).ok(),
            content_hash(&crlf.content).ok()
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
            content_hash(&files_a.content).ok(),
            content_hash(&files_b.content).ok()
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

    fn set_time(storage: &ClipboardStorage, id: i64, at: i64) {
        if let Err(e) = storage.conn.execute(
            "UPDATE clipboard_item SET last_copied_at = ?1 WHERE id = ?2",
            params![at, id],
        ) {
            panic!("failed to set last_copied_at: {}", e);
        }
    }

    fn list_ids(storage: &ClipboardStorage, query_text: &str, filter: ClipFilter) -> Vec<i64> {
        match storage.list(query_text, filter, 50, 0) {
            Ok((items, _)) => items.into_iter().map(|item| item.id).collect(),
            Err(e) => panic!("list failed: {}", e),
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 空库列表返回空数组与零总数() {
        let storage = open_test_storage();
        match storage.list("", ClipFilter::All, 50, 0) {
            Ok((items, total)) => {
                assert!(items.is_empty());
                assert_eq!(total, 0);
            }
            Err(e) => panic!("list on empty db failed: {}", e),
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 列表按置顶与最近复制时间排序() {
        let mut storage = open_test_storage();
        let first = match storage.insert(&text_entry("第一条")) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };
        let second = match storage.insert(&text_entry("第二条")) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };
        let third = match storage.insert(&text_entry("第三条")) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };

        set_time(&storage, first.id(), 1_000);
        set_time(&storage, second.id(), 3_000);
        set_time(&storage, third.id(), 2_000);

        assert_eq!(
            list_ids(&storage, "", ClipFilter::All),
            vec![second.id(), third.id(), first.id()]
        );

        // 置顶项无论时间多旧都在最前
        if let Err(e) = storage.set_pinned(first.id(), true) {
            panic!("set_pinned failed: {}", e);
        }
        assert_eq!(
            list_ids(&storage, "", ClipFilter::All),
            vec![first.id(), second.id(), third.id()]
        );
    }

    #[test]
    #[allow(non_snake_case)]
    fn 相同复制时间按id倒序稳定() {
        let mut storage = open_test_storage();
        let first = match storage.insert(&text_entry("并列一")) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };
        let second = match storage.insert(&text_entry("并列二")) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };
        set_time(&storage, first.id(), 5_000);
        set_time(&storage, second.id(), 5_000);

        assert_eq!(
            list_ids(&storage, "", ClipFilter::All),
            vec![second.id(), first.id()]
        );
    }

    #[test]
    #[allow(non_snake_case)]
    fn 筛选覆盖文本图片文件与收藏() {
        let mut storage = open_test_storage();
        let text = match storage.insert(&text_entry("普通文本")) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };
        let html = match storage.insert(&ClipboardEntry {
            content: ClipboardContent::Html("<b>富文本</b>".to_string()),
            preview: "富文本".to_string(),
            truncated: false,
            source_app: None,
            source_title: None,
        }) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };
        let files = match storage.insert(&ClipboardEntry {
            content: ClipboardContent::Files(vec!["C:\\a.txt".to_string()]),
            preview: "1 个文件".to_string(),
            truncated: false,
            source_app: None,
            source_title: None,
        }) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };

        assert_eq!(
            list_ids(&storage, "", ClipFilter::Text),
            vec![html.id(), text.id()]
        );
        assert!(list_ids(&storage, "", ClipFilter::Image).is_empty());
        assert_eq!(list_ids(&storage, "", ClipFilter::Files), vec![files.id()]);

        if let Err(e) = storage.set_pinned(files.id(), true) {
            panic!("set_pinned failed: {}", e);
        }
        assert_eq!(
            list_ids(&storage, "", ClipFilter::Favorites),
            vec![files.id()]
        );
    }

    #[test]
    #[allow(non_snake_case)]
    fn 分页总数与页大小一致() {
        let mut storage = open_test_storage();
        for index in 0..5 {
            let entry = text_entry(&format!("分页条目{}", index));
            if let Err(e) = storage.insert(&entry) {
                panic!("insert failed: {}", e);
            }
        }

        match storage.list("", ClipFilter::All, 2, 0) {
            Ok((items, total)) => {
                assert_eq!(items.len(), 2);
                assert_eq!(total, 5);
            }
            Err(e) => panic!("list failed: {}", e),
        }

        match storage.list("", ClipFilter::All, 2, 4) {
            Ok((items, total)) => {
                assert_eq!(items.len(), 1);
                assert_eq!(total, 5);
            }
            Err(e) => panic!("list failed: {}", e),
        }

        match storage.list("", ClipFilter::All, 2, 100) {
            Ok((items, total)) => {
                assert!(items.is_empty());
                assert_eq!(total, 5);
            }
            Err(e) => panic!("list failed: {}", e),
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 详情返回正文且不存在时为空() {
        let mut storage = open_test_storage();
        let outcome = match storage.insert(&text_entry("详情正文内容")) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };

        match storage.get(outcome.id()) {
            Ok(Some(detail)) => {
                assert_eq!(detail.text_body.as_deref(), Some("详情正文内容"));
                assert_eq!(detail.preview, "详情正文内容");
            }
            Ok(None) => panic!("detail should exist"),
            Err(e) => panic!("get failed: {}", e),
        }

        match storage.get(99_999) {
            Ok(None) => {}
            Ok(Some(_)) => panic!("detail should not exist"),
            Err(e) => panic!("get failed: {}", e),
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 删除同时清理fts且不存在时报错() {
        let mut storage = open_test_storage();
        let outcome = match storage.insert(&text_entry("待删除内容")) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };

        if let Err(e) = storage.delete(outcome.id(), false) {
            panic!("delete failed: {}", e);
        }

        let fts_rows: i64 = match storage.conn.query_row(
            "SELECT COUNT(*) FROM clipboard_fts WHERE rowid = ?1",
            params![outcome.id()],
            |row| row.get(0),
        ) {
            Ok(count) => count,
            Err(e) => panic!("failed to count fts rows: {}", e),
        };
        assert_eq!(fts_rows, 0);
        assert_eq!(count_items(&storage), 0);

        assert!(storage.delete(outcome.id(), false).is_err());
    }

    #[test]
    #[allow(non_snake_case)]
    fn fts搜索时置顶项仍在最前() {
        let mut storage = open_test_storage();
        let plain = match storage.insert(&text_entry("会议纪要草稿")) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };
        let pinned = match storage.insert(&text_entry("会议纪要终稿")) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };

        set_time(&storage, plain.id(), 9_000);
        set_time(&storage, pinned.id(), 1_000);
        if let Err(e) = storage.set_pinned(pinned.id(), true) {
            panic!("set_pinned failed: {}", e);
        }

        let ids = list_ids(&storage, "会议纪要", ClipFilter::All);
        assert_eq!(ids.first().copied(), Some(pinned.id()));
        assert_eq!(ids.len(), 2);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 只读打开未迁移的库会先建表() {
        let dir = std::env::temp_dir().join(format!("quickwin-readonly-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db_path = dir.join("quickwin.db");

        let storage = match ClipboardStorage::open_readonly_at(&db_path) {
            Ok(storage) => storage,
            Err(e) => panic!("readonly open failed: {}", e),
        };

        match storage.list("", ClipFilter::All, 50, 0) {
            Ok((items, total)) => {
                assert!(items.is_empty());
                assert_eq!(total, 0);
            }
            Err(e) => panic!("list on fresh db failed: {}", e),
        }

        drop(storage);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 硬删除同时移除blob文件() {
        let mut storage = open_test_storage();
        let outcome = match storage.insert(&ClipboardEntry {
            content: ClipboardContent::Image {
                data: vec![0x89, 0x50, 0x4e, 0x47],
                format: ImageFormat::Png,
            },
            preview: "图片".to_string(),
            truncated: false,
            source_app: None,
            source_title: None,
        }) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };

        let relative: Option<String> = match storage.conn.query_row(
            "SELECT blob_path FROM clipboard_item WHERE id = ?1",
            params![outcome.id()],
            |row| row.get(0),
        ) {
            Ok(value) => value,
            Err(e) => panic!("failed to read blob_path: {}", e),
        };
        let relative = match relative {
            Some(path) => path,
            None => panic!("image item should have blob_path"),
        };
        let absolute = storage.data_dir.join(&relative);
        assert!(absolute.exists(), "blob should be written before delete");

        if let Err(e) = storage.delete(outcome.id(), true) {
            panic!("delete failed: {}", e);
        }
        assert!(!absolute.exists(), "hard delete must remove blob file");
    }

    #[test]
    #[allow(non_snake_case)]
    fn 千条数据搜索查询在五十毫秒内() {
        use std::time::{Duration, Instant};

        let mut storage = open_test_storage();
        for index in 0..1000 {
            let entry = text_entry(&format!(
                "会议纪要 第 {} 条，正文内容 sample body {}",
                index, index
            ));
            if let Err(e) = storage.insert(&entry) {
                panic!("seed insert failed: {}", e);
            }
        }

        // 预热：排除首次 prepare 的开销
        let _ = storage.list("会议纪要", ClipFilter::All, 50, 0);
        let _ = storage.list("会", ClipFilter::All, 50, 0);

        let mut fts = Vec::new();
        let mut like = Vec::new();
        for _ in 0..10 {
            let start = Instant::now();
            if let Err(e) = storage.list("会议纪要", ClipFilter::All, 50, 0) {
                panic!("fts query failed: {}", e);
            }
            fts.push(start.elapsed());

            let start = Instant::now();
            if let Err(e) = storage.list("会", ClipFilter::All, 50, 0) {
                panic!("like query failed: {}", e);
            }
            like.push(start.elapsed());
        }

        fts.sort();
        like.sort();
        let fts_median = fts[fts.len() / 2];
        let like_median = like[like.len() / 2];

        assert!(
            fts_median < Duration::from_millis(50),
            "FTS median too slow: {:?}",
            fts_median
        );
        assert!(
            like_median < Duration::from_millis(50),
            "LIKE median too slow: {:?}",
            like_median
        );
    }

    #[test]
    #[allow(non_snake_case)]
    fn 纯标点查询回退like不报错() {
        let mut storage = open_test_storage();
        if let Err(e) = storage.insert(&text_entry("100% 完成")) {
            panic!("insert failed: {}", e);
        }

        match storage.list("...", ClipFilter::All, 50, 0) {
            Ok((items, total)) => {
                assert!(items.is_empty());
                assert_eq!(total, 0);
            }
            Err(e) => panic!("punctuation query failed: {}", e),
        }
    }
}
