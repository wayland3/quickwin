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
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
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
const SCHEMA_VERSION: i64 = 4;

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

/// v2 schema：设置表（单条 JSON）
const SCHEMA_V2: &str = r#"
CREATE TABLE settings (
    id    INTEGER PRIMARY KEY CHECK (id = 1),
    value TEXT NOT NULL
);
"#;

/// v3 schema：应用索引（来源 start_menu / app_paths / uwp / builtin）
const SCHEMA_V3: &str = r#"
CREATE TABLE app_index (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    key        TEXT NOT NULL UNIQUE,
    name       TEXT NOT NULL,
    target     TEXT NOT NULL,
    args       TEXT,
    kind       TEXT NOT NULL CHECK (kind IN ('win32','uwp','url','command')),
    source     TEXT NOT NULL,
    icon_path  TEXT,
    updated_at INTEGER NOT NULL
);
CREATE INDEX ix_app_name ON app_index(name);
"#;

/// v4 schema：使用频率（frecency）
const SCHEMA_V4: &str = r#"
CREATE TABLE usage_stat (
    item_key     TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    use_count    INTEGER NOT NULL DEFAULT 0,
    last_used_at INTEGER NOT NULL
);
"#;

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

/// 清理结果
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CleanupReport {
    /// 删除的条目数
    pub deleted_items: usize,
    /// 删除的 blob 文件数
    pub deleted_blobs: usize,
    /// 回收的孤儿 blob 数
    pub reclaimed_orphans: usize,
}

/// 清空历史结果
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClearReport {
    pub deleted_items: usize,
    pub deleted_blobs: usize,
}

/// 应用索引行（写入 `app_index` 表）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIndexRow {
    /// 去重键：小写可执行文件路径或小写 AUMID
    pub key: String,
    /// 显示名
    pub name: String,
    /// 目标（可执行文件路径或 AUMID）
    pub target: String,
    /// 启动参数
    pub args: Option<String>,
    /// win32 / uwp / url / command
    pub kind: String,
    /// start_menu / app_paths / uwp / builtin
    pub source: String,
    /// 图标位置（`path` 或 `path,index`）
    pub icon_path: Option<String>,
}

/// 使用频率行（`usage_stat`）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageStat {
    pub key: String,
    pub kind: String,
    pub use_count: i64,
    pub last_used_at: i64,
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
    /// 测试用：内存库的临时 blob 目录需要在 drop 时清理
    #[cfg(test)]
    cleanup_dir: bool,
}

#[cfg(test)]
impl Drop for ClipboardStorage {
    fn drop(&mut self) {
        if self.cleanup_dir {
            let _ = std::fs::remove_dir_all(&self.data_dir);
        }
    }
}

impl ClipboardStorage {
    /// 创建生产环境存储实例    ///
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

        let mut storage = Self {
            conn,
            data_dir,
            #[cfg(test)]
            cleanup_dir: false,
        };
        storage.migrate()?;
        storage.check_fts_consistency()?;

        Ok(storage)
    }

    /// 内存数据库（仅测试使用）
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, AppError> {
        use std::sync::atomic::{AtomicU64, Ordering};

        // 每个实例独立目录：并行测试会同时写 blob，共享目录会导致清理互相干扰
        static TEST_DIR_SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let data_dir =
            std::env::temp_dir().join(format!("quickwin-test-{}-{}", std::process::id(), seq));
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| AppError::Storage(format!("failed to create test directory: {}", e)))?;

        let conn = Connection::open_in_memory()
            .map_err(|e| AppError::Storage(format!("failed to open in-memory database: {}", e)))?;
        Self::configure(&conn)?;

        let mut storage = Self {
            conn,
            data_dir,
            cleanup_dir: true,
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
    ///
    /// busy_timeout 必须先设：后续 journal_mode 切换需要排他锁，先有重试保护才能避免
    /// 并发首开时拿到不吃 busy handler 的硬 SQLITE_BUSY。
    fn configure(conn: &Connection) -> Result<(), AppError> {
        conn.execute_batch(
            "PRAGMA busy_timeout = 5000;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )
        .map_err(|e| AppError::Storage(format!("failed to set pragmas: {}", e)))?;

        // journal_mode 返回一行结果，必须用 query_row 而不是 execute
        conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| AppError::Storage(format!("failed to set journal_mode: {}", e)))?;

        Ok(())
    }

    /// 执行迁移：按版本逐级升级（v0→v1→v2…），禁止跳级
    fn migrate(&mut self) -> Result<(), AppError> {
        let mut version = self.read_user_version()?;
        if version > SCHEMA_VERSION {
            return Err(AppError::Storage(format!(
                "unsupported schema version: {}",
                version
            )));
        }

        while version < SCHEMA_VERSION {
            match version {
                0 => {
                    self.apply_version(1, SCHEMA_V1)?;
                    log::info!("database migrated: version=1");
                }
                1 => {
                    self.apply_version(2, SCHEMA_V2)?;
                    log::info!("database migrated: version=2");
                }
                2 => {
                    self.apply_version(3, SCHEMA_V3)?;
                    log::info!("database migrated: version=3");
                }
                3 => {
                    self.apply_version(4, SCHEMA_V4)?;
                    log::info!("database migrated: version=4");
                }
                other => {
                    return Err(AppError::Storage(format!(
                        "missing migration for version: {}",
                        other
                    )))
                }
            }
            version = self.read_user_version()?;
        }

        Ok(())
    }

    /// 应用单个版本：独立事务执行 SQL 并更新 user_version
    fn apply_version(&mut self, target: i64, sql: &str) -> Result<(), AppError> {
        let tx = self.conn.transaction().map_err(|e| {
            AppError::Storage(format!("failed to begin migration v{}: {}", target, e))
        })?;

        tx.execute_batch(sql)
            .map_err(|e| AppError::Storage(format!("failed to apply schema v{}: {}", target, e)))?;

        tx.pragma_update(None, "user_version", target)
            .map_err(|e| {
                AppError::Storage(format!("failed to set user_version v{}: {}", target, e))
            })?;

        tx.commit().map_err(|e| {
            AppError::Storage(format!("failed to commit migration v{}: {}", target, e))
        })
    }

    /// 启动时校验 FTS 与主表行数一致，不一致则重建
    ///
    /// 单语句内同时取两个 COUNT，保证是同一快照；若拆成两条自动提交读，中间插入提交
    /// 会造成 main=n、fts=n+1 的瞬时不一致，从而误触发全量 rebuild。
    fn check_fts_consistency(&self) -> Result<(), AppError> {
        let consistent: bool = self
            .conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM clipboard_item)
                        = (SELECT COUNT(*) FROM clipboard_fts)",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Storage(format!("failed to check FTS consistency: {}", e)))?;

        if consistent {
            return Ok(());
        }

        log::warn!("FTS inconsistency detected, rebuilding");
        self.conn
            .execute(
                "INSERT INTO clipboard_fts(clipboard_fts) VALUES('rebuild')",
                [],
            )
            .map_err(|e| AppError::Storage(format!("failed to rebuild FTS: {}", e)))?;

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

        // IMMEDIATE：事务开始即排队拿写锁，避免多连接 DEFERRED 先持 SHARED 锁后
        // 升级写锁造成死锁（该路径不触发 busy_timeout，直接 database is locked）
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
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
            .transaction_with_behavior(TransactionBehavior::Immediate)
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

    /// 按保留策略清理：TTL → 条数上限 → blob 预算（置顶全部豁免）
    ///
    /// 删除主表 + FTS + blob 文件；返回删除数量。清理后回收超过 1 小时的孤儿 blob。
    pub fn cleanup(
        &mut self,
        policy: &crate::services::retention::RetentionPolicy,
        now_ms: i64,
    ) -> Result<CleanupReport, AppError> {
        let mut report = CleanupReport::default();

        // 1) TTL 过期（按 last_copied_at）
        let mut ids: Vec<i64> = Vec::new();
        if let Some(cutoff) = policy.ttl_cutoff(now_ms) {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM clipboard_item WHERE pinned = 0 AND last_copied_at < ?1")
                .map_err(|e| AppError::Storage(format!("prepare ttl query: {}", e)))?;
            let rows = stmt
                .query_map(params![cutoff], |row| row.get::<_, i64>(0))
                .map_err(|e| AppError::Storage(format!("run ttl query: {}", e)))?;
            for row in rows {
                ids.push(row.map_err(|e| AppError::Storage(format!("read ttl id: {}", e)))?);
            }
        }

        // 2) 条数上限（只淘汰非置顶，最久未使用优先）
        let total = self
            .conn
            .query_row("SELECT COUNT(*) FROM clipboard_item", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|e| AppError::Storage(format!("count items: {}", e)))?;
        let excess = policy.excess_items(total);
        if excess > 0 {
            let mut stmt = self
                .conn
                .prepare(&format!(
                    "SELECT id FROM clipboard_item WHERE pinned = 0 \
                     ORDER BY last_copied_at ASC, id ASC LIMIT {}",
                    excess
                ))
                .map_err(|e| AppError::Storage(format!("prepare excess query: {}", e)))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, i64>(0))
                .map_err(|e| AppError::Storage(format!("run excess query: {}", e)))?;
            for row in rows {
                ids.push(row.map_err(|e| AppError::Storage(format!("read excess id: {}", e)))?);
            }
        }

        ids.sort_unstable();
        ids.dedup();
        if !ids.is_empty() {
            let blobs = self.delete_ids(&ids)?;
            report.deleted_items = ids.len();
            report.deleted_blobs = blobs.len();
            self.remove_blob_files(&blobs);
        }

        // 3) blob 总预算（超限时淘汰最旧的非置顶图片条目）
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, byte_size, blob_path FROM clipboard_item \
                 WHERE pinned = 0 AND kind = 'image' AND blob_path IS NOT NULL \
                 ORDER BY last_copied_at ASC, id ASC",
            )
            .map_err(|e| AppError::Storage(format!("prepare blob query: {}", e)))?;
        let blob_rows: Vec<(i64, i64, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| AppError::Storage(format!("run blob query: {}", e)))?
            .filter_map(Result::ok)
            .collect();
        drop(stmt);

        // 按磁盘实际占用判定（包含孤儿文件），与验收「把 blob 目录塞到超限」一致
        let mut remaining = self.blob_usage_on_disk()?;
        if remaining > policy.blob_budget {
            let mut overflow: Vec<i64> = Vec::new();
            for (id, size, _) in &blob_rows {
                if remaining <= policy.blob_budget {
                    break;
                }
                overflow.push(*id);
                remaining = (remaining - *size).max(0);
            }
            if !overflow.is_empty() {
                let blobs = self.delete_ids(&overflow)?;
                report.deleted_items += overflow.len();
                report.deleted_blobs += blobs.len();
                self.remove_blob_files(&blobs);
            }
        }

        // 4) 孤儿 blob 回收（仅删除 1 小时前创建的）
        report.reclaimed_orphans = self.reclaim_orphan_blobs(now_ms)?;

        log::info!(
            "clipboard cleanup: items={}, blobs={}, orphans={}",
            report.deleted_items,
            report.deleted_blobs,
            report.reclaimed_orphans
        );
        Ok(report)
    }

    /// 清空历史：`include_pinned=false` 保留收藏（置顶）项
    pub fn clear_history(&mut self, include_pinned: bool) -> Result<ClearReport, AppError> {
        let mut report = ClearReport::default();

        let mut stmt = self
            .conn
            .prepare(if include_pinned {
                "SELECT id FROM clipboard_item"
            } else {
                "SELECT id FROM clipboard_item WHERE pinned = 0"
            })
            .map_err(|e| AppError::Storage(format!("prepare clear query: {}", e)))?;
        let ids: Vec<i64> = stmt
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(|e| AppError::Storage(format!("run clear query: {}", e)))?
            .filter_map(Result::ok)
            .collect();
        drop(stmt);

        if !ids.is_empty() {
            let blobs = self.delete_ids(&ids)?;
            report.deleted_items = ids.len();
            report.deleted_blobs = blobs.len();
            self.remove_blob_files(&blobs);
        }

        // FTS 与主表行数对齐（防御性 rebuild，避免残留行）
        self.check_fts_consistency()?;

        // 全部清空后 blobs/ 不应有残留（含孤儿文件）
        if include_pinned {
            let blobs_dir = self.data_dir.join("blobs");
            if blobs_dir.exists() {
                if let Err(e) = std::fs::remove_dir_all(&blobs_dir) {
                    log::warn!("failed to remove blobs dir: {}", e);
                }
            }
        }

        log::info!(
            "clipboard cleared: items={}, blobs={}, include_pinned={}",
            report.deleted_items,
            report.deleted_blobs,
            include_pinned
        );
        Ok(report)
    }

    /// blobs 目录实际磁盘占用（字节），包含孤儿文件
    fn blob_usage_on_disk(&self) -> Result<i64, AppError> {
        let blobs_dir = self.data_dir.join("blobs");
        if !blobs_dir.exists() {
            return Ok(0);
        }

        let mut total = 0i64;
        for shard in std::fs::read_dir(&blobs_dir)
            .map_err(|e| AppError::Storage(format!("read blobs dir: {}", e)))?
            .filter_map(Result::ok)
        {
            let shard_path = shard.path();
            if !shard_path.is_dir() {
                if let Ok(meta) = shard.metadata() {
                    total += meta.len() as i64;
                }
                continue;
            }
            for entry in std::fs::read_dir(&shard_path)
                .map_err(|e| AppError::Storage(format!("read shard dir: {}", e)))?
                .filter_map(Result::ok)
            {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        total += meta.len() as i64;
                    }
                }
            }
        }
        Ok(total)
    }

    /// 回收孤儿 blob：不在 `blob_path` 引用中且修改时间超过 `ORPHAN_MIN_AGE_MS`
    pub fn reclaim_orphan_blobs(&self, now_ms: i64) -> Result<usize, AppError> {
        let mut referenced = std::collections::HashSet::new();
        let mut stmt = self
            .conn
            .prepare("SELECT blob_path FROM clipboard_item WHERE blob_path IS NOT NULL")
            .map_err(|e| AppError::Storage(format!("prepare blob paths: {}", e)))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| AppError::Storage(format!("run blob paths: {}", e)))?;
        for row in rows.flatten() {
            referenced.insert(row.replace('\\', "/"));
        }
        drop(stmt);

        let mut removed = 0usize;
        let blobs_dir = self.data_dir.join("blobs");
        if !blobs_dir.exists() {
            return Ok(0);
        }

        for shard in std::fs::read_dir(&blobs_dir)
            .map_err(|e| AppError::Storage(format!("read blobs dir: {}", e)))?
            .filter_map(Result::ok)
        {
            let shard_path = shard.path();
            if !shard_path.is_dir() {
                continue;
            }
            for entry in std::fs::read_dir(&shard_path)
                .map_err(|e| AppError::Storage(format!("read shard dir: {}", e)))?
                .filter_map(Result::ok)
            {
                let file_path = entry.path();
                if !file_path.is_file() {
                    continue;
                }
                let relative = file_path
                    .strip_prefix(&self.data_dir)
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                if referenced.contains(&relative) {
                    continue;
                }

                let age_ok = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| {
                        now_ms - d.as_millis() as i64
                            >= crate::services::retention::ORPHAN_MIN_AGE_MS
                    })
                    .unwrap_or(false);
                if age_ok {
                    match std::fs::remove_file(&file_path) {
                        Ok(()) => removed += 1,
                        Err(e) => log::warn!("failed to remove orphan blob: {}", e),
                    }
                }
            }
        }

        Ok(removed)
    }

    /// 事务内按 id 删除主表与 FTS，返回被删条目的 blob 相对路径
    fn delete_ids(&mut self, ids: &[i64]) -> Result<Vec<String>, AppError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| AppError::Storage(format!("begin cleanup tx: {}", e)))?;

        let mut blobs = Vec::new();
        {
            let mut stmt = tx
                .prepare("SELECT blob_path FROM clipboard_item WHERE id = ?1")
                .map_err(|e| AppError::Storage(format!("prepare blob lookup: {}", e)))?;
            for id in ids {
                let blob: Option<Option<String>> = stmt
                    .query_row(params![id], |row| row.get::<_, Option<String>>(0))
                    .optional()
                    .map_err(|e| AppError::Storage(format!("query blob: {}", e)))?;
                if let Some(Some(path)) = blob {
                    blobs.push(path);
                }
            }
        }

        {
            let mut fts = tx
                .prepare("DELETE FROM clipboard_fts WHERE rowid = ?1")
                .map_err(|e| AppError::Storage(format!("prepare fts delete: {}", e)))?;
            let mut items = tx
                .prepare("DELETE FROM clipboard_item WHERE id = ?1")
                .map_err(|e| AppError::Storage(format!("prepare item delete: {}", e)))?;
            for id in ids {
                fts.execute(params![id])
                    .map_err(|e| AppError::Storage(format!("delete fts: {}", e)))?;
                items
                    .execute(params![id])
                    .map_err(|e| AppError::Storage(format!("delete item: {}", e)))?;
            }
        }

        tx.commit()
            .map_err(|e| AppError::Storage(format!("commit cleanup: {}", e)))?;
        Ok(blobs)
    }

    /// 删除 blob 文件（失败只记日志，不影响 DB 结果）
    fn remove_blob_files(&self, blobs: &[String]) {
        for relative in blobs {
            let absolute = self.data_dir.join(relative);
            if let Err(e) = std::fs::remove_file(&absolute) {
                log::warn!("failed to remove blob file: {}", e);
            }
        }
    }

    /// 全量替换应用索引（单事务 DELETE + INSERT；失败自动回滚，保留旧索引）
    ///
    /// 重复 key 防御性跳过（扫描层已去重）；返回实际写入条数。
    pub fn replace_app_index(
        &mut self,
        rows: &[AppIndexRow],
        updated_at: i64,
    ) -> Result<usize, AppError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| {
                AppError::Storage(format!("failed to begin app index transaction: {}", e))
            })?;

        tx.execute("DELETE FROM app_index", [])
            .map_err(|e| AppError::Storage(format!("failed to clear app index: {}", e)))?;

        let mut inserted = 0usize;
        let mut seen: HashSet<String> = HashSet::new();
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO app_index
                        (key, name, target, args, kind, source, icon_path, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                )
                .map_err(|e| {
                    AppError::Storage(format!("failed to prepare app index insert: {}", e))
                })?;

            for row in rows {
                let key = row.key.trim().to_lowercase();
                if key.is_empty() || !seen.insert(key.clone()) {
                    continue;
                }
                stmt.execute(params![
                    key,
                    row.name,
                    row.target,
                    row.args,
                    row.kind,
                    row.source,
                    row.icon_path,
                    updated_at
                ])
                .map_err(|e| AppError::Storage(format!("failed to insert app index row: {}", e)))?;
                inserted += 1;
            }
        }

        tx.commit()
            .map_err(|e| AppError::Storage(format!("failed to commit app index: {}", e)))?;

        Ok(inserted)
    }

    /// 读取全部使用频率（frecency 排序用）
    pub fn load_usage_stats(&self) -> Result<Vec<UsageStat>, AppError> {
        let mut stmt = self
            .conn
            .prepare("SELECT item_key, kind, use_count, last_used_at FROM usage_stat")
            .map_err(|e| AppError::Storage(format!("failed to prepare usage query: {}", e)))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(UsageStat {
                    key: row.get(0)?,
                    kind: row.get(1)?,
                    use_count: row.get(2)?,
                    last_used_at: row.get(3)?,
                })
            })
            .map_err(|e| AppError::Storage(format!("failed to query usage: {}", e)))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(
                row.map_err(|e| AppError::Storage(format!("failed to read usage row: {}", e)))?,
            );
        }
        Ok(out)
    }

    /// 记录一次使用（不存在则插入，存在则次数 +1 并刷新时间）
    pub fn bump_usage(&mut self, key: &str, kind: &str, now_ms: i64) -> Result<(), AppError> {
        self.conn
            .execute(
                "INSERT INTO usage_stat (item_key, kind, use_count, last_used_at)
                 VALUES (?1, ?2, 1, ?3)
                 ON CONFLICT(item_key) DO UPDATE SET
                   use_count = usage_stat.use_count + 1,
                   last_used_at = ?3",
                params![key, kind, now_ms],
            )
            .map_err(|e| AppError::Storage(format!("failed to bump usage: {}", e)))?;
        Ok(())
    }

    /// 读取全部应用索引行（按 key 排序，便于比较与测试）
    pub fn load_app_index(&self) -> Result<Vec<AppIndexRow>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT key, name, target, args, kind, source, icon_path FROM app_index ORDER BY key",
            )
            .map_err(|e| AppError::Storage(format!("failed to prepare app index query: {}", e)))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(AppIndexRow {
                    key: row.get(0)?,
                    name: row.get(1)?,
                    target: row.get(2)?,
                    args: row.get(3)?,
                    kind: row.get(4)?,
                    source: row.get(5)?,
                    icon_path: row.get(6)?,
                })
            })
            .map_err(|e| AppError::Storage(format!("failed to query app index: {}", e)))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(
                row.map_err(|e| AppError::Storage(format!("failed to read app index row: {}", e)))?,
            );
        }
        Ok(out)
    }

    /// 按 key 读取单条应用索引行（启动器执行用）
    pub fn load_app(&self, key: &str) -> Result<Option<AppIndexRow>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT key, name, target, args, kind, source, icon_path FROM app_index WHERE key = ?1",
            )
            .map_err(|e| AppError::Storage(format!("failed to prepare app lookup: {}", e)))?;

        let mut rows = stmt
            .query_map(params![key.to_lowercase()], |row| {
                Ok(AppIndexRow {
                    key: row.get(0)?,
                    name: row.get(1)?,
                    target: row.get(2)?,
                    args: row.get(3)?,
                    kind: row.get(4)?,
                    source: row.get(5)?,
                    icon_path: row.get(6)?,
                })
            })
            .map_err(|e| AppError::Storage(format!("failed to query app lookup: {}", e)))?;

        match rows.next() {
            Some(row) => {
                let row =
                    row.map_err(|e| AppError::Storage(format!("failed to read app row: {}", e)))?;
                Ok(Some(row))
            }
            None => Ok(None),
        }
    }

    /// 打开可写连接（业务写命令用）：跳过启动期 FTS 一致性全表计数
    ///
    /// 一致性校验只在应用启动的监听线程初始化时执行一次；置顶/删除等高频路径不应
    /// 每次打开都做表级检查。版本必须已是当前版本，未迁移的库不在业务写路径处理。
    pub fn open_write_at(db_path: &Path) -> Result<Self, AppError> {
        let data_dir = db_path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| AppError::Storage("invalid database path".to_string()))?;

        let conn = Connection::open(db_path)
            .map_err(|e| AppError::Storage(format!("failed to open database: {}", e)))?;
        Self::configure(&conn)?;

        let storage = Self {
            conn,
            data_dir,
            #[cfg(test)]
            cleanup_dir: false,
        };
        let version = storage.read_user_version()?;
        if version != SCHEMA_VERSION {
            return Err(AppError::Storage(format!(
                "write connection requires migrated database, version: {}",
                version
            )));
        }

        Ok(storage)
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

        let mut storage = Self {
            conn,
            data_dir,
            #[cfg(test)]
            cleanup_dir: false,
        };
        let version = storage.read_user_version()?;

        if version < SCHEMA_VERSION {
            // 并发首读时另一个连接可能已抢先完成迁移：失败后再读一次版本，已到当前版本则放行
            if let Err(err) = storage.migrate() {
                if storage.read_user_version()? < SCHEMA_VERSION {
                    return Err(err);
                }
            }
        } else if version > SCHEMA_VERSION {
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

    /// 读取设置；表为空或解析失败时回退默认（不写库，不阻断启动）
    pub fn load_settings(&self) -> crate::models::Settings {
        use crate::models::Settings;

        let result: Result<Option<String>, _> = self
            .conn
            .query_row("SELECT value FROM settings WHERE id = 1", [], |row| {
                row.get(0)
            })
            .optional();

        match result {
            Ok(Some(json)) => match serde_json::from_str::<Settings>(&json) {
                Ok(settings) => match settings.validate() {
                    Ok(()) => settings,
                    Err(reason) => {
                        log::warn!("stored settings invalid, using defaults: {}", reason);
                        Settings::default()
                    }
                },
                Err(e) => {
                    log::warn!("failed to parse stored settings, using defaults: {}", e);
                    Settings::default()
                }
            },
            Ok(None) => Settings::default(),
            Err(e) => {
                log::warn!("failed to read settings, using defaults: {}", e);
                Settings::default()
            }
        }
    }

    /// 写入设置（UPSERT 单条 JSON）
    pub fn save_settings(&self, settings: &crate::models::Settings) -> Result<(), AppError> {
        let json = serde_json::to_string(settings)
            .map_err(|e| AppError::Storage(format!("failed to serialize settings: {}", e)))?;

        self.conn
            .execute(
                "INSERT INTO settings (id, value) VALUES (1, ?1)
                 ON CONFLICT(id) DO UPDATE SET value = excluded.value",
                params![json],
            )
            .map_err(|e| AppError::Storage(format!("failed to save settings: {}", e)))?;
        Ok(())
    }

    /// 事务内的读-改-写设置：BEGIN IMMEDIATE 保证不丢更新
    ///
    /// 返回 `(旧设置, 新设置, 变化字段)`；字段无变化时不写库。
    /// 校验由调用方在事务外先做（这里只负责合并与持久化）。
    pub fn transaction_update_settings<F>(
        &mut self,
        merge: F,
    ) -> Result<
        (
            crate::models::Settings,
            crate::models::Settings,
            Vec<String>,
        ),
        AppError,
    >
    where
        F: FnOnce(&crate::models::Settings) -> Result<crate::models::Settings, AppError>,
    {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| AppError::Storage(format!("failed to begin settings tx: {}", e)))?;

        // 事务内重读，避免读到事务外的过期值
        let current: crate::models::Settings = {
            let result: Result<Option<String>, _> = tx
                .query_row("SELECT value FROM settings WHERE id = 1", [], |row| {
                    row.get(0)
                })
                .optional();
            match result {
                Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_default(),
                _ => crate::models::Settings::default(),
            }
        };

        let next = merge(&current)?;
        let changed = diff_settings_keys(&current, &next);

        if !changed.is_empty() {
            let json = serde_json::to_string(&next)
                .map_err(|e| AppError::Storage(format!("failed to serialize settings: {}", e)))?;
            tx.execute(
                "INSERT INTO settings (id, value) VALUES (1, ?1)
                 ON CONFLICT(id) DO UPDATE SET value = excluded.value",
                params![json],
            )
            .map_err(|e| AppError::Storage(format!("failed to save settings: {}", e)))?;
        }

        tx.commit()
            .map_err(|e| AppError::Storage(format!("failed to commit settings: {}", e)))?;
        Ok((current, next, changed))
    }
}

/// 比较前后设置，返回发生变化的字段名（事务更新用）
pub fn diff_settings_keys(
    previous: &crate::models::Settings,
    next: &crate::models::Settings,
) -> Vec<String> {
    let prev_json = serde_json::to_value(previous).ok();
    let next_json = serde_json::to_value(next).ok();

    let mut keys = Vec::new();
    if let (Some(prev), Some(next)) = (prev_json, next_json) {
        if let (Some(prev_obj), Some(next_obj)) = (prev.as_object(), next.as_object()) {
            let mut all: Vec<&String> = prev_obj.keys().chain(next_obj.keys()).collect();
            all.sort();
            all.dedup();
            for key in all {
                if prev_obj.get(key) != next_obj.get(key) {
                    keys.push(key.clone());
                }
            }
        }
    }
    keys
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
        #[cfg(target_os = "windows")]
        let format = ImageFormat::Dib;
        #[cfg(not(target_os = "windows"))]
        let format = ImageFormat::Raw;
        let entry = ClipboardEntry {
            content: ClipboardContent::Image {
                data: Vec::new(),
                format,
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

    #[test]
    #[allow(non_snake_case)]
    fn v1库迁移到最新版本后剪贴板数据保留() {
        let unique = format!(
            "quickwin-migrate-v2-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let db_path = std::env::temp_dir().join(unique).join("quickwin.db");

        // 用普通连接手工建 v1 库（不走 open_at，否则会直接迁移到最新版）
        let dir_path = db_path.parent().expect("db parent");
        std::fs::create_dir_all(dir_path).expect("create db dir");
        let v1_conn = rusqlite::Connection::open(&db_path).expect("open v1 connection");
        v1_conn.execute_batch(SCHEMA_V1).expect("apply v1 schema");
        v1_conn
            .pragma_update(None, "user_version", 1)
            .expect("set v1 version");

        // 插入一条剪贴板数据
        let hash = "v1-migrate-hash".to_string();
        v1_conn
            .execute(
                "INSERT INTO clipboard_item (
                    kind, hash, preview, byte_size, created_at, last_copied_at
                 ) VALUES ('text', ?1, '迁移保留文本', 6, 1000, 1000)",
                params![hash],
            )
            .expect("insert v1 row");
        drop(v1_conn);

        let version_before: i64 = {
            let conn = rusqlite::Connection::open(&db_path).expect("open for version");
            conn.query_row("PRAGMA user_version", [], |row| row.get(0))
                .expect("read version")
        };
        assert_eq!(version_before, 1);

        // 打开时自动迁移到最新版本
        let storage = ClipboardStorage::open_at(&db_path).expect("migrate to latest");
        let version_after: i64 = storage
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("read migrated version");
        assert_eq!(version_after, SCHEMA_VERSION);

        let table_exists: i64 = storage
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='settings'",
                [],
                |row| row.get(0),
            )
            .expect("check settings table");
        assert_eq!(table_exists, 1);

        let app_index_exists: i64 = storage
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='app_index'",
                [],
                |row| row.get(0),
            )
            .expect("check app_index table");
        assert_eq!(app_index_exists, 1);

        let (_items, total) = storage
            .list("", ClipFilter::All, 50, 0)
            .expect("list after migration");
        assert_eq!(total, 1);

        let integrity: String = storage
            .conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap_or_default();
        assert_eq!(integrity, "ok");

        if let Some(dir) = db_path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    fn app_row(key: &str, name: &str) -> AppIndexRow {
        AppIndexRow {
            key: key.to_string(),
            name: name.to_string(),
            target: key.to_string(),
            args: None,
            kind: "win32".to_string(),
            source: "start_menu".to_string(),
            icon_path: None,
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn v2库迁移到v3后设置与剪贴板数据保留() {
        let unique = format!(
            "quickwin-migrate-v3-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let db_path = std::env::temp_dir().join(unique).join("quickwin.db");

        let dir_path = db_path.parent().expect("db parent");
        std::fs::create_dir_all(dir_path).expect("create db dir");
        let v2_conn = rusqlite::Connection::open(&db_path).expect("open v2 connection");
        v2_conn.execute_batch(SCHEMA_V1).expect("apply v1 schema");
        v2_conn.execute_batch(SCHEMA_V2).expect("apply v2 schema");
        v2_conn
            .pragma_update(None, "user_version", 2)
            .expect("set v2 version");

        v2_conn
            .execute(
                "INSERT INTO clipboard_item (
                    kind, hash, preview, byte_size, created_at, last_copied_at
                 ) VALUES ('text', 'v2-migrate-hash', '迁移保留文本', 6, 1000, 1000)",
                [],
            )
            .expect("insert v2 clipboard row");
        let settings_json =
            serde_json::to_string(&crate::models::Settings::default()).expect("serialize settings");
        v2_conn
            .execute(
                "INSERT INTO settings (id, value) VALUES (1, ?1)",
                params![settings_json],
            )
            .expect("insert v2 settings");
        drop(v2_conn);

        let storage = ClipboardStorage::open_at(&db_path).expect("migrate to v3");
        let version_after: i64 = storage
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("read migrated version");
        assert_eq!(version_after, SCHEMA_VERSION);

        let (_items, total) = storage
            .list("", ClipFilter::All, 50, 0)
            .expect("list after migration");
        assert_eq!(total, 1);

        let loaded = storage.load_settings();
        assert_eq!(loaded, crate::models::Settings::default());

        let app_index_exists: i64 = storage
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='app_index'",
                [],
                |row| row.get(0),
            )
            .expect("check app_index table");
        assert_eq!(app_index_exists, 1);

        if let Some(dir) = db_path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 应用索引全量替换且重复键被跳过() {
        let mut storage = open_test_storage();

        let first = vec![app_row("a.exe", "Alpha"), app_row("b.exe", "Beta")];
        assert_eq!(
            storage
                .replace_app_index(&first, 1000)
                .expect("replace app index"),
            2
        );

        // 第二次替换清掉旧数据；大小写不同的同一 key 只保留首次出现
        let second = vec![
            app_row("B.EXE", "Beta 新"),
            app_row("b.exe", "Beta 重复"),
            app_row("c.exe", "Gamma"),
        ];
        assert_eq!(
            storage
                .replace_app_index(&second, 2000)
                .expect("replace app index again"),
            2
        );

        let mut stmt = storage
            .conn
            .prepare("SELECT key, name, updated_at FROM app_index ORDER BY key")
            .expect("prepare app index query");
        let rows: Vec<(String, String, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .expect("query app index")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect app index");

        assert_eq!(
            rows,
            vec![
                ("b.exe".to_string(), "Beta 新".to_string(), 2000),
                ("c.exe".to_string(), "Gamma".to_string(), 2000),
            ]
        );

        let loaded = storage.load_app_index().expect("load app index");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].key, "b.exe");
        assert_eq!(loaded[1].key, "c.exe");
    }

    #[test]
    #[allow(non_snake_case)]
    fn 按key读取单条应用且大小写不敏感() {
        let mut storage = open_test_storage();
        let rows = vec![app_row("a.exe", "Alpha"), app_row("b.exe", "Beta")];
        storage
            .replace_app_index(&rows, 1000)
            .expect("seed app index");

        let found = storage.load_app("A.EXE").expect("load app").expect("found");
        assert_eq!(found.key, "a.exe");
        assert_eq!(found.name, "Alpha");
        assert_eq!(found.kind, "win32");

        assert!(storage
            .load_app("missing.exe")
            .expect("load absent")
            .is_none());
    }

    #[test]
    #[allow(non_snake_case)]
    fn 空列表清空应用索引() {
        let mut storage = open_test_storage();
        let rows = vec![app_row("a.exe", "Alpha")];
        storage
            .replace_app_index(&rows, 1000)
            .expect("seed app index");

        assert_eq!(
            storage
                .replace_app_index(&[], 2000)
                .expect("clear app index"),
            0
        );

        let count: i64 = storage
            .conn
            .query_row("SELECT COUNT(*) FROM app_index", [], |row| row.get(0))
            .expect("count app index");
        assert_eq!(count, 0);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 一千条应用索引替换在两秒内() {
        let mut storage = open_test_storage();
        let rows: Vec<AppIndexRow> = (0..1000)
            .map(|index| app_row(&format!("app-{index}.exe"), &format!("应用 {index}")))
            .collect();

        let started = std::time::Instant::now();
        let inserted = storage
            .replace_app_index(&rows, 1000)
            .expect("replace 1000 rows");
        let elapsed = started.elapsed();

        assert_eq!(inserted, 1000);
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "1000 条替换耗时 {:?}，超过 2s 预算",
            elapsed
        );
    }

    #[test]
    #[allow(non_snake_case)]
    fn v3库迁移到v4后应用索引保留() {
        let unique = format!(
            "quickwin-migrate-v4-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let db_path = std::env::temp_dir().join(unique).join("quickwin.db");

        let dir_path = db_path.parent().expect("db parent");
        std::fs::create_dir_all(dir_path).expect("create db dir");
        let v3_conn = rusqlite::Connection::open(&db_path).expect("open v3 connection");
        for schema in [SCHEMA_V1, SCHEMA_V2, SCHEMA_V3] {
            v3_conn.execute_batch(schema).expect("apply schema");
        }
        v3_conn
            .pragma_update(None, "user_version", 3)
            .expect("set v3 version");
        v3_conn
            .execute(
                "INSERT INTO app_index (key, name, target, kind, source, updated_at)
                 VALUES ('a.exe', 'Alpha', 'a.exe', 'win32', 'start_menu', 1000)",
                [],
            )
            .expect("insert app row");
        drop(v3_conn);

        let storage = ClipboardStorage::open_at(&db_path).expect("migrate to v4");
        let version_after: i64 = storage
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("read migrated version");
        assert_eq!(version_after, SCHEMA_VERSION);

        let usage_exists: i64 = storage
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='usage_stat'",
                [],
                |row| row.get(0),
            )
            .expect("check usage_stat table");
        assert_eq!(usage_exists, 1);

        let apps = storage.load_app_index().expect("load app index");
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].key, "a.exe");

        if let Some(dir) = db_path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 使用频率累加且刷新时间() {
        let mut storage = open_test_storage();
        storage
            .bump_usage("a.exe", "app", 1000)
            .expect("first bump");
        storage
            .bump_usage("a.exe", "app", 2000)
            .expect("second bump");

        let stats = storage.load_usage_stats().expect("load usage");
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].key, "a.exe");
        assert_eq!(stats[0].use_count, 2);
        assert_eq!(stats[0].last_used_at, 2000);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 并发写入查询无锁错误且数据一致() {
        use std::thread;

        let unique = format!(
            "quickwin-concurrent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let db_path = std::env::temp_dir().join(unique).join("quickwin.db");

        // 先初始化 schema，避免多个线程同时对空库迁移
        let initializer = match ClipboardStorage::open_at(&db_path) {
            Ok(storage) => storage,
            Err(e) => panic!("initializer failed: {}", e),
        };
        drop(initializer);

        let handles: Vec<thread::JoinHandle<Result<u32, AppError>>> = (0..5)
            .map(|worker| {
                let path = db_path.clone();
                thread::spawn(move || -> Result<u32, AppError> {
                    let mut storage = ClipboardStorage::open_write_at(&path)?;
                    let mut stored = 0u32;
                    for index in 0..20 {
                        let entry = text_entry(&format!("并发 worker {} 条目 {}", worker, index));
                        storage.insert(&entry)?;
                        stored += 1;

                        // 每次写入后在独立只读连接上查询：5 线程 × 20 = 100 次并发查询
                        let readonly = ClipboardStorage::open_readonly_at(&path)?;
                        let (items, _total) = readonly.list("并发", ClipFilter::All, 50, 0)?;
                        let _count = items.len();
                    }
                    Ok(stored)
                })
            })
            .collect();

        let mut total_stored = 0u32;
        for handle in handles {
            match handle.join() {
                Ok(Ok(count)) => total_stored += count,
                Ok(Err(e)) => panic!("worker failed (expected no database is locked): {:?}", e),
                Err(_) => panic!("worker thread panicked"),
            }
        }
        assert_eq!(total_stored, 100);

        // 重启后数据一致
        let storage = match ClipboardStorage::open_at(&db_path) {
            Ok(storage) => storage,
            Err(e) => panic!("reopen failed: {}", e),
        };
        let (_items, total) = match storage.list("", ClipFilter::All, 200, 0) {
            Ok(result) => result,
            Err(e) => panic!("count after reopen failed: {}", e),
        };
        assert_eq!(total, 100);

        let integrity: String = storage
            .conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap_or_default();
        assert_eq!(integrity, "ok");

        if let Some(dir) = db_path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 事务更新失败时回滚不落库() {
        let mut storage = ClipboardStorage::open_in_memory().expect("open in-memory storage");
        let initial = storage.load_settings();

        let result = storage.transaction_update_settings(|current| {
            let _ = current;
            Err::<crate::models::Settings, crate::error::AppError>(
                crate::error::AppError::SettingsInvalid("模拟失败".to_string()),
            )
        });
        assert!(result.is_err());

        let after = storage.load_settings();
        assert_eq!(after, initial);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 事务更新无变化时不写库() {
        let mut storage = ClipboardStorage::open_in_memory().expect("open in-memory storage");
        let initial = storage.load_settings();
        storage.save_settings(&initial).expect("save initial");

        let (current, next, changed) = storage
            .transaction_update_settings(|current| Ok(current.clone()))
            .expect("tx update");
        assert_eq!(current, initial);
        assert_eq!(next, initial);
        assert!(changed.is_empty());
    }

    #[test]
    #[allow(non_snake_case)]
    fn 事务更新成功时写入并返回变化字段() {
        let mut storage = ClipboardStorage::open_in_memory().expect("open in-memory storage");

        let (current, next, changed) = storage
            .transaction_update_settings(|current| {
                let mut next = current.clone();
                next.max_items = 5000;
                next.theme = "dark".to_string();
                Ok(next)
            })
            .expect("tx update");

        assert_eq!(current.max_items, 10_000);
        assert_eq!(next.max_items, 5000);
        assert!(changed.contains(&"max_items".to_string()));
        assert!(changed.contains(&"theme".to_string()));

        let loaded = storage.load_settings();
        assert_eq!(loaded.max_items, 5000);
        assert_eq!(loaded.theme, "dark");
    }

    fn image_entry_bytes(data: Vec<u8>) -> ClipboardEntry {
        ClipboardEntry {
            content: ClipboardContent::Image {
                data,
                format: ImageFormat::Png,
            },
            preview: "[图片]".to_string(),
            truncated: false,
            source_app: None,
            source_title: None,
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn TTL过期清理且置顶豁免() {
        let mut storage = open_test_storage();
        let day = 24 * 60 * 60 * 1000i64;

        let old = match storage.insert(&text_entry("过期条目")) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };
        let pinned = match storage.insert(&text_entry("过期但置顶")) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };
        let fresh = match storage.insert(&text_entry("最近条目")) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };

        set_time(&storage, old.id(), day);
        set_time(&storage, pinned.id(), day);
        set_time(&storage, fresh.id(), 20 * day);
        if let Err(e) = storage.set_pinned(pinned.id(), true) {
            panic!("set_pinned failed: {}", e);
        }

        let policy = crate::services::retention::RetentionPolicy {
            ttl_days: 15,
            max_items: 10_000,
            blob_budget: crate::services::retention::BLOB_BUDGET_BYTES,
        };
        match storage.cleanup(&policy, 20 * day) {
            Ok(report) => assert_eq!(report.deleted_items, 1),
            Err(e) => panic!("cleanup failed: {:?}", e),
        }

        let ids = list_ids(&storage, "", ClipFilter::All);
        assert!(!ids.contains(&old.id()));
        assert!(ids.contains(&pinned.id()));
        assert!(ids.contains(&fresh.id()));
    }

    #[test]
    #[allow(non_snake_case)]
    fn 超过条数上限淘汰最旧且置顶豁免() {
        let mut storage = open_test_storage();
        let mut inserted = Vec::new();
        for index in 0..12 {
            let outcome = match storage.insert(&text_entry(&format!("条数条目{}", index))) {
                Ok(o) => o,
                Err(e) => panic!("insert failed: {}", e),
            };
            set_time(&storage, outcome.id(), i64::from(index) + 1);
            inserted.push(outcome.id());
        }
        // 置顶最旧项，验证不被淘汰
        if let Err(e) = storage.set_pinned(inserted[0], true) {
            panic!("set_pinned failed: {}", e);
        }

        let policy = crate::services::retention::RetentionPolicy {
            ttl_days: -1,
            max_items: 10,
            blob_budget: crate::services::retention::BLOB_BUDGET_BYTES,
        };
        match storage.cleanup(&policy, 1_000_000) {
            Ok(report) => assert_eq!(report.deleted_items, 2),
            Err(e) => panic!("cleanup failed: {:?}", e),
        }

        let ids = list_ids(&storage, "", ClipFilter::All);
        assert_eq!(ids.len(), 10);
        assert!(ids.contains(&inserted[0]));
        assert!(!ids.contains(&inserted[1]));
        assert!(!ids.contains(&inserted[2]));
    }

    #[test]
    #[allow(non_snake_case)]
    fn blob超预算淘汰最旧图片并删文件() {
        let unique = format!(
            "quickwin-blob-budget-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let db_path = std::env::temp_dir().join(unique).join("quickwin.db");
        let mut storage = match ClipboardStorage::open_at(&db_path) {
            Ok(s) => s,
            Err(e) => panic!("open failed: {}", e),
        };

        let mut blobs = Vec::new();
        for index in 0..3u8 {
            let data = vec![index; 100];
            let outcome = match storage.insert(&image_entry_bytes(data)) {
                Ok(o) => o,
                Err(e) => panic!("insert failed: {}", e),
            };
            set_time(&storage, outcome.id(), i64::from(index) + 1);
            blobs.push(outcome.id());
        }

        let policy = crate::services::retention::RetentionPolicy {
            ttl_days: -1,
            max_items: 10_000,
            blob_budget: 200,
        };
        match storage.cleanup(&policy, 1_000_000) {
            Ok(report) => {
                assert_eq!(report.deleted_items, 1);
                assert_eq!(report.deleted_blobs, 1);
            }
            Err(e) => panic!("cleanup failed: {:?}", e),
        }

        let stored: Vec<String> = {
            let mut stmt = match storage
                .conn
                .prepare("SELECT blob_path FROM clipboard_item WHERE blob_path IS NOT NULL")
            {
                Ok(stmt) => stmt,
                Err(e) => panic!("prepare failed: {}", e),
            };
            let rows = match stmt.query_map([], |row| row.get::<_, String>(0)) {
                Ok(rows) => rows,
                Err(e) => panic!("query failed: {}", e),
            };
            rows.filter_map(Result::ok).collect()
        };
        assert_eq!(stored.len(), 2);
        for relative in &stored {
            assert!(storage.data_dir.join(relative).exists());
        }

        if let Some(dir) = db_path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 清空历史可保留收藏() {
        let mut storage = open_test_storage();
        let plain = match storage.insert(&text_entry("普通条目")) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };
        let pinned = match storage.insert(&text_entry("收藏条目")) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };
        if let Err(e) = storage.set_pinned(pinned.id(), true) {
            panic!("set_pinned failed: {}", e);
        }

        match storage.clear_history(false) {
            Ok(report) => assert_eq!(report.deleted_items, 1),
            Err(e) => panic!("clear failed: {:?}", e),
        }
        assert_eq!(count_items(&storage), 1);
        assert!(list_ids(&storage, "", ClipFilter::Favorites).contains(&pinned.id()));

        match storage.clear_history(true) {
            Ok(report) => assert_eq!(report.deleted_items, 1),
            Err(e) => panic!("clear all failed: {}", e),
        }
        assert_eq!(count_items(&storage), 0);
        let fts_rows: i64 =
            match storage
                .conn
                .query_row("SELECT COUNT(*) FROM clipboard_fts", [], |row| row.get(0))
            {
                Ok(count) => count,
                Err(e) => panic!("count fts failed: {}", e),
            };
        assert_eq!(fts_rows, 0);
        assert!(!list_ids(&storage, "", ClipFilter::All).contains(&plain.id()));
    }

    #[test]
    #[allow(non_snake_case)]
    fn 孤儿blob只回收过期文件() {
        let unique = format!(
            "quickwin-orphan-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let db_path = std::env::temp_dir().join(unique).join("quickwin.db");
        let mut storage = match ClipboardStorage::open_at(&db_path) {
            Ok(s) => s,
            Err(e) => panic!("open failed: {}", e),
        };

        let outcome = match storage.insert(&image_entry_bytes(vec![1, 2, 3])) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };
        let referenced: Option<String> = match storage.conn.query_row(
            "SELECT blob_path FROM clipboard_item WHERE id = ?1",
            params![outcome.id()],
            |row| row.get(0),
        ) {
            Ok(path) => path,
            Err(e) => panic!("query blob failed: {}", e),
        };
        let referenced = match referenced {
            Some(path) => path,
            None => panic!("image should have blob"),
        };

        let orphan_dir = storage.data_dir.join("blobs").join("zz");
        if let Err(e) = std::fs::create_dir_all(&orphan_dir) {
            panic!("create orphan dir failed: {}", e);
        }
        let orphan = orphan_dir.join("orphan.png");
        if let Err(e) = std::fs::write(&orphan, [0u8; 4]) {
            panic!("write orphan failed: {}", e);
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
            + 2 * 60 * 60 * 1000;
        match storage.reclaim_orphan_blobs(now_ms) {
            Ok(removed) => assert_eq!(removed, 1),
            Err(e) => panic!("reclaim failed: {}", e),
        }
        assert!(!orphan.exists());
        assert!(storage.data_dir.join(&referenced).exists());

        if let Some(dir) = db_path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn blob预算按磁盘实际占用判定() {
        let unique = format!(
            "quickwin-blob-disk-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let db_path = std::env::temp_dir().join(unique).join("quickwin.db");
        let mut storage = match ClipboardStorage::open_at(&db_path) {
            Ok(s) => s,
            Err(e) => panic!("open failed: {}", e),
        };

        let outcome = match storage.insert(&image_entry_bytes(vec![9u8; 100])) {
            Ok(o) => o,
            Err(e) => panic!("insert failed: {}", e),
        };
        set_time(&storage, outcome.id(), 1);
        let blob_path: String = match storage.conn.query_row(
            "SELECT blob_path FROM clipboard_item WHERE id = ?1",
            params![outcome.id()],
            |row| row.get(0),
        ) {
            Ok(path) => path,
            Err(e) => panic!("query blob failed: {}", e),
        };

        // 额外写入 300 字节孤儿文件，使磁盘占用超过预算
        let orphan_dir = storage.data_dir.join("blobs").join("zz");
        if let Err(e) = std::fs::create_dir_all(&orphan_dir) {
            panic!("create orphan dir failed: {}", e);
        }
        if let Err(e) = std::fs::write(orphan_dir.join("new-orphan.png"), [7u8; 300]) {
            panic!("write orphan failed: {}", e);
        }

        let policy = crate::services::retention::RetentionPolicy {
            ttl_days: -1,
            max_items: 10_000,
            blob_budget: 200,
        };
        match storage.cleanup(&policy, 1_000_000) {
            Ok(report) => {
                assert_eq!(report.deleted_items, 1);
                assert_eq!(report.deleted_blobs, 1);
            }
            Err(e) => panic!("cleanup failed: {:?}", e),
        }

        assert!(!storage.data_dir.join(&blob_path).exists());

        if let Some(dir) = db_path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 全部清空后blobs目录为空() {
        let unique = format!(
            "quickwin-clear-blobs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let db_path = std::env::temp_dir().join(unique).join("quickwin.db");
        let mut storage = match ClipboardStorage::open_at(&db_path) {
            Ok(s) => s,
            Err(e) => panic!("open failed: {}", e),
        };

        match storage.insert(&image_entry_bytes(vec![1, 2, 3])) {
            Ok(_) => {}
            Err(e) => panic!("insert failed: {}", e),
        }
        let blobs_dir = storage.data_dir.join("blobs");
        assert!(blobs_dir.exists());

        match storage.clear_history(true) {
            Ok(report) => assert_eq!(report.deleted_items, 1),
            Err(e) => panic!("clear failed: {:?}", e),
        }
        assert!(!blobs_dir.exists());

        if let Some(dir) = db_path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
