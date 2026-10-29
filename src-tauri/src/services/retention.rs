// 保留策略与容量治理
//
// 纯策略计算（TTL 截止点、条数/容量上限）与清理时机常量。
// 实际操作 SQL 与 blob 文件删除在 `services/storage.rs`。

use crate::models::Settings;

/// blob 总预算（500MB，PRD 6.5 固定值，不随设置变化）
pub const BLOB_BUDGET_BYTES: i64 = 500 * 1024 * 1024;

/// 定时清理间隔：6 小时
pub const CLEANUP_INTERVAL_SECS: u64 = 6 * 60 * 60;

/// 每累计多少次写入触发一次清理
pub const CLEANUP_WRITE_THRESHOLD: u32 = 200;

/// 孤儿 blob 最小年龄（1 小时，避免删掉刚写入尚未入库的文件）
pub const ORPHAN_MIN_AGE_MS: i64 = 60 * 60 * 1000;

/// 保留策略（由设置派生）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// TTL 天数：-1 表示永不
    pub ttl_days: i32,
    /// 条数上限
    pub max_items: i64,
    /// blob 总预算（字节）
    pub blob_budget: i64,
}

impl RetentionPolicy {
    /// 从设置构造策略
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            ttl_days: settings.retention_days,
            max_items: i64::from(settings.max_items),
            blob_budget: BLOB_BUDGET_BYTES,
        }
    }

    /// TTL 截止时间（毫秒）；`None` 表示永不清理
    pub fn ttl_cutoff(&self, now_ms: i64) -> Option<i64> {
        if self.ttl_days <= 0 {
            return None;
        }
        Some(now_ms - i64::from(self.ttl_days) * 24 * 60 * 60 * 1000)
    }

    /// 需要淘汰的条数（总数超过上限时）
    ///
    /// 置顶豁免，最多只能淘汰非置顶条目，因此实际删除数可能小于该值。
    pub fn excess_items(&self, total_items: i64) -> i64 {
        (total_items - self.max_items).max(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(ttl: i32, max_items: i32) -> Settings {
        Settings {
            retention_days: ttl,
            max_items,
            ..Settings::default()
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn TTL截止时间按天数计算() {
        let policy = RetentionPolicy::from_settings(&settings(15, 10_000));
        let now = 1_700_000_000_000i64;
        assert_eq!(policy.ttl_cutoff(now), Some(now - 15 * 24 * 60 * 60 * 1000));
    }

    #[test]
    #[allow(non_snake_case)]
    fn 永不清理时截止时间为空() {
        let policy = RetentionPolicy::from_settings(&settings(-1, 10_000));
        assert_eq!(policy.ttl_cutoff(1_700_000_000_000), None);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 条数超出计算正确() {
        let policy = RetentionPolicy::from_settings(&settings(15, 1_000));
        assert_eq!(policy.excess_items(1_005), 5);
        assert_eq!(policy.excess_items(1_000), 0);
        assert_eq!(policy.excess_items(900), 0);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 默认策略参数来自PRD() {
        let policy = RetentionPolicy::from_settings(&Settings::default());
        assert_eq!(policy.ttl_days, 15);
        assert_eq!(policy.max_items, 10_000);
        assert_eq!(policy.blob_budget, 500 * 1024 * 1024);
    }
}
