//! 应用匹配与排序（纯函数，无 IO）。

use crate::models::{LauncherItem, LauncherKind};
use crate::services::storage::{AppIndexRow, UsageStat};
use std::collections::HashMap;

/// 默认结果条数（SPEC §5.2）
pub const DEFAULT_LIMIT: usize = 12;
/// 命令层允许的上限
pub const MAX_LIMIT: usize = 20;

/// 匹配档位（分数见 ARCHITECTURE §8.1）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    Exact,
    Prefix,
    Acronym,
    Substring,
    Fuzzy,
}

impl MatchKind {
    fn score(self) -> f64 {
        match self {
            Self::Exact => 1.0,
            Self::Prefix => 0.9,
            Self::Acronym => 0.85,
            Self::Substring => 0.7,
            Self::Fuzzy => 0.4,
        }
    }
}

/// 匹配查询串；无命中返回 `None`
pub fn match_query(query: &str, title: &str) -> Option<(MatchKind, f64)> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some((MatchKind::Exact, MatchKind::Exact.score()));
    }
    let title_lower = title.to_lowercase();

    if title_lower == query {
        return Some((MatchKind::Exact, MatchKind::Exact.score()));
    }
    if title_lower.starts_with(&query) {
        return Some((MatchKind::Prefix, MatchKind::Prefix.score()));
    }
    if acronym(title).starts_with(&query) {
        return Some((MatchKind::Acronym, MatchKind::Acronym.score()));
    }
    if title_lower.contains(&query) {
        return Some((MatchKind::Substring, MatchKind::Substring.score()));
    }
    fuzzy_score(&query, &title_lower).map(|score| (MatchKind::Fuzzy, score))
}

/// 词首缩写：`Visual Studio Code` → `vsc`
pub fn acronym(title: &str) -> String {
    title
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .filter_map(|word| word.chars().next())
        .filter(|ch| ch.is_alphabetic())
        .flat_map(char::to_lowercase)
        .collect()
}

/// 子序列模糊匹配：分数随匹配紧密度在 0.4–0.6 之间
fn fuzzy_score(query: &str, title_lower: &str) -> Option<f64> {
    let query_chars: Vec<char> = query.chars().collect();
    let mut target = query_chars.iter();
    let mut current = target.next()?;
    let mut first: Option<usize> = None;
    let mut last = 0usize;
    let mut matched = 0usize;

    for (index, ch) in title_lower.chars().enumerate() {
        if ch == *current {
            if first.is_none() {
                first = Some(index);
            }
            last = index;
            matched += 1;
            match target.next() {
                Some(next) => current = next,
                None => break,
            }
        }
    }

    if matched != query_chars.len() {
        return None;
    }
    let first = first?;
    let span = last.saturating_sub(first) + 1;
    let density = matched as f64 / span as f64;
    Some(0.4 + 0.2 * density)
}

const MS_PER_DAY: f64 = 86_400_000.0;

/// frecency = 1 + ln(1 + use_count) × 0.5^(days / 14)
pub fn frecency(use_count: i64, last_used_at: i64, now_ms: i64) -> f64 {
    if use_count <= 0 {
        return 1.0;
    }
    let days = (now_ms - last_used_at).max(0) as f64 / MS_PER_DAY;
    1.0 + (1.0 + use_count as f64).ln() * 0.5f64.powf(days / 14.0)
}

/// 来源权重（ARCHITECTURE §8.1）
pub fn source_weight(source: &str) -> f64 {
    match source {
        "start_menu" => 1.0,
        "app_paths" => 0.95,
        "uwp" => 0.9,
        "builtin" => 1.05,
        _ => 1.0,
    }
}

/// 应用排序：匹配分 × frecency × 来源权重，同分按标题长度、再按 key
pub fn rank_apps(
    query: &str,
    rows: &[AppIndexRow],
    usage: &HashMap<String, UsageStat>,
    now_ms: i64,
    limit: usize,
) -> Vec<LauncherItem> {
    let normalized = query.trim().to_lowercase();
    let mut items = Vec::new();

    for row in rows {
        let match_score = if normalized.is_empty() {
            1.0
        } else {
            match match_query(&normalized, &row.name) {
                Some((_, score)) => score,
                None => continue,
            }
        };

        let (use_count, last_used_at) = usage
            .get(&row.key)
            .map(|stat| (stat.use_count, stat.last_used_at))
            .unwrap_or((0, 0));
        let weight = match_score * frecency(use_count, last_used_at, now_ms);
        let score = weight * source_weight(&row.source);

        items.push(LauncherItem {
            key: row.key.clone(),
            title: row.name.clone(),
            subtitle: Some(subtitle_for(row)),
            kind: LauncherKind::App,
            icon: row.icon_path.clone(),
            score: score as f32,
        });
    }

    items.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.title.chars().count().cmp(&right.title.chars().count()))
            .then_with(|| left.key.cmp(&right.key))
    });
    items.truncate(limit.clamp(1, MAX_LIMIT));
    items
}

fn subtitle_for(row: &AppIndexRow) -> String {
    match row.kind.as_str() {
        "uwp" => "UWP 应用".to_string(),
        _ => row.target.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str, name: &str, source: &str) -> AppIndexRow {
        AppIndexRow {
            key: key.to_string(),
            name: name.to_string(),
            target: key.to_string(),
            args: None,
            kind: "win32".to_string(),
            source: source.to_string(),
            icon_path: None,
        }
    }

    fn usage(key: &str, count: i64, last: i64) -> UsageStat {
        UsageStat {
            key: key.to_string(),
            kind: "app".to_string(),
            use_count: count,
            last_used_at: last,
        }
    }

    #[test]
    fn 完全匹配优先于前缀与子串() {
        let exact = match_query("chrome", "Chrome").expect("exact");
        let prefix = match_query("chr", "Chrome").expect("prefix");
        let substring = match_query("rome", "Chrome").expect("substring");

        assert_eq!(exact.0, MatchKind::Exact);
        assert_eq!(prefix.0, MatchKind::Prefix);
        assert_eq!(substring.0, MatchKind::Substring);
        assert!(exact.1 > prefix.1 && prefix.1 > substring.1);
    }

    #[test]
    fn 首字母缩写命中多词标题() {
        let result = match_query("vsc", "Visual Studio Code").expect("acronym");
        assert_eq!(result.0, MatchKind::Acronym);
        assert_eq!(acronym("Visual Studio Code"), "vsc");
    }

    #[test]
    fn 子序列模糊命中按紧密度给分() {
        let tight = match_query("vso", "vscode").expect("fuzzy tight");
        let loose = match_query("vso", "visual studio code").expect("fuzzy loose");

        assert_eq!(tight.0, MatchKind::Fuzzy);
        assert!(tight.1 > 0.4);
        assert!(tight.1 >= loose.1);
        assert!(match_query("xyz", "Chrome").is_none());
    }

    #[test]
    fn 空查询视为全量命中() {
        let result = match_query("  ", "Chrome").expect("empty");
        assert_eq!(result.0, MatchKind::Exact);
        assert_eq!(result.1, 1.0);
    }

    #[test]
    fn frecency随次数与时间衰减() {
        let now = 1_000_000_000_000i64;
        let fresh = frecency(10, now - 86_400_000, now);
        let stale = frecency(10, now - 90 * 86_400_000, now);
        let unused = frecency(0, 0, now);

        assert!(fresh > stale);
        assert_eq!(unused, 1.0);
        assert!(frecency(5, now, now) > frecency(1, now, now));
    }

    #[test]
    fn 常用应用排在使用少的之前() {
        let now = 1_000_000_000_000i64;
        let rows = vec![
            row("a.exe", "Alpha", "start_menu"),
            row("b.exe", "Alpha", "start_menu"),
        ];
        let mut stats = HashMap::new();
        stats.insert("b.exe".to_string(), usage("b.exe", 8, now));

        let items = rank_apps("alp", &rows, &stats, now, 12);
        assert_eq!(items[0].key, "b.exe");
        assert_eq!(items[1].key, "a.exe");
    }

    #[test]
    fn 来源权重开始菜单高于uwp() {
        let now = 1_000_000_000_000i64;
        let rows = vec![
            row("a.exe", "Same", "start_menu"),
            row("aumid!app", "Same", "uwp"),
        ];
        let items = rank_apps("same", &rows, &HashMap::new(), now, 12);
        assert_eq!(items[0].key, "a.exe");
    }

    #[test]
    fn 结果上限为十二且同分稳定() {
        let now = 1_000_000_000_000i64;
        let rows: Vec<AppIndexRow> = (0..30)
            .map(|index| row(&format!("app-{index}.exe"), "Same", "start_menu"))
            .collect();
        let items = rank_apps("same", &rows, &HashMap::new(), now, 12);
        assert_eq!(items.len(), 12);
        assert_eq!(items[0].key, "app-0.exe");
    }

    #[test]
    fn 无匹配时返回空列表() {
        let now = 1_000_000_000_000i64;
        let items = rank_apps(
            "zzz",
            &[row("a.exe", "Chrome", "start_menu")],
            &HashMap::new(),
            now,
            12,
        );

        assert!(items.is_empty());
    }

    #[test]
    fn 空查询按frecency排序() {
        let now = 1_000_000_000_000i64;
        let rows = vec![
            row("a.exe", "Alpha", "start_menu"),
            row("b.exe", "Beta", "start_menu"),
        ];
        let mut stats = HashMap::new();
        stats.insert("a.exe".to_string(), usage("a.exe", 3, now));

        let items = rank_apps("", &rows, &stats, now, 12);
        assert_eq!(items[0].key, "a.exe");
    }

    /// 真机抽查：`cargo test --lib 真机排序抽查 -- --ignored --nocapture`
    #[test]
    #[ignore = "仅真机手测运行"]
    fn 真机排序抽查() {
        let scan = crate::services::appindex::collect();
        let usage = HashMap::new();
        let now = 1_700_000_000_000i64;
        for query in ["fir", "vsc", "微信"] {
            let items = rank_apps(query, &scan.rows, &usage, now, 6);
            let titles: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
            println!("query {query}: {titles:?}");
        }
    }
}
