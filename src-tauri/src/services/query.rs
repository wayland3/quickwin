// 剪贴板搜索条件构造（纯函数，便于单测）
//
// 规则来源：
// - `PRD §6.3`：中文 1 字起即时过滤（1–2 字回退 LIKE，≥3 字走 FTS）
// - `ARCHITECTURE §5.3`：FTS 查询串用双引号包裹并转义内部引号；LIKE 用 ESCAPE '\'
//
// 纯标点查询不能走 FTS：trigram 分词器对纯 ASCII 标点不产生有效 token，
// 短语 MATCH 会报语法错误，因此先判「是否含可搜索字符」再判长度。

use crate::models::ClipFilter;

/// 搜索路径
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    /// 无搜索条件
    None,
    /// FTS5 MATCH（≥3 个可搜索字符）
    Fts,
    /// LIKE 回退（1–2 字符或纯标点）
    Like,
}

/// 搜索条件：路径 + 已转义的参数
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSpec {
    pub kind: SearchKind,
    /// FTS 时为加引号的短语；LIKE 时为已转义并加 `%...%` 的模式；None 时为空串
    pub param: String,
}

/// 构造搜索条件
pub fn build_search(query: &str) -> SearchSpec {
    let trimmed = query.trim();

    if trimmed.is_empty() {
        return SearchSpec {
            kind: SearchKind::None,
            param: String::new(),
        };
    }

    // 纯标点 / 纯符号：FTS trigram 无有效 token，必须回退 LIKE
    if !contains_searchable_char(trimmed) {
        return SearchSpec {
            kind: SearchKind::Like,
            param: build_like_pattern(trimmed),
        };
    }

    if trimmed.chars().count() >= 3 {
        SearchSpec {
            kind: SearchKind::Fts,
            param: build_fts_phrase(trimmed),
        }
    } else {
        SearchSpec {
            kind: SearchKind::Like,
            param: build_like_pattern(trimmed),
        }
    }
}

/// 是否含字母、数字或汉字（Unicode 字母/数字即可覆盖 CJK）
fn contains_searchable_char(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

/// FTS 短语：双引号包裹，内部 `"` 转义为 `""`
fn build_fts_phrase(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

/// LIKE 模式：转义 `\` `%` `_`，再用 `%...%` 包裹
fn build_like_pattern(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len() + 2);
    for ch in text.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    format!("%{}%", escaped)
}

/// 筛选条件片段（拼进 WHERE，固定字符串，无注入面）
pub fn filter_clause(filter: ClipFilter) -> &'static str {
    match filter {
        ClipFilter::All => "",
        ClipFilter::Text => " AND kind IN ('text','html','rtf')",
        ClipFilter::Image => " AND kind = 'image'",
        ClipFilter::Files => " AND kind = 'files'",
        ClipFilter::Favorites => " AND pinned = 1",
    }
}

/// FTS 联表查询用的筛选片段（列需要 `c.` 前缀）
pub fn filter_clause_fts(filter: ClipFilter) -> &'static str {
    match filter {
        ClipFilter::All => "",
        ClipFilter::Text => " AND c.kind IN ('text','html','rtf')",
        ClipFilter::Image => " AND c.kind = 'image'",
        ClipFilter::Files => " AND c.kind = 'files'",
        ClipFilter::Favorites => " AND c.pinned = 1",
    }
}

/// limit 夹取到 1..=200；返回夹取后的值与是否被夹取
pub fn clamp_limit(limit: u32) -> (u32, bool) {
    let clamped = limit.clamp(1, 200);
    (clamped, clamped != limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_is_none() {
        for q in ["", "   ", "\t\n"] {
            let spec = build_search(q);
            assert_eq!(spec.kind, SearchKind::None);
            assert!(spec.param.is_empty());
        }
    }

    #[test]
    fn short_queries_use_like() {
        for q in ["a", "ab", "中", "中文"] {
            assert_eq!(build_search(q).kind, SearchKind::Like, "query={}", q);
        }
    }

    #[test]
    fn three_or_more_chars_use_fts() {
        for q in ["abc", "中文搜", "hello"] {
            assert_eq!(build_search(q).kind, SearchKind::Fts, "query={}", q);
        }
    }

    #[test]
    fn plain_punctuation_falls_back_to_like() {
        for q in ["...", "!!!", "%%%", "___", "——", "   ...   "] {
            let spec = build_search(q);
            assert_eq!(spec.kind, SearchKind::Like, "query={}", q);
        }
    }

    #[test]
    fn fts_phrase_escapes_quotes() {
        let spec = build_search("say \"hi\" now");
        assert_eq!(spec.kind, SearchKind::Fts);
        assert_eq!(spec.param, "\"say \"\"hi\"\" now\"");
    }

    #[test]
    fn like_pattern_escapes_wildcards() {
        assert_eq!(build_search("a%").param, r"%a\%%");
        assert_eq!(build_search("a_").param, r"%a\_%");
        assert_eq!(build_search("\\").param, r"%\\%");
    }

    #[test]
    fn filter_clauses_are_stable() {
        assert_eq!(filter_clause(ClipFilter::All), "");
        assert_eq!(
            filter_clause(ClipFilter::Text),
            " AND kind IN ('text','html','rtf')"
        );
        assert_eq!(filter_clause(ClipFilter::Image), " AND kind = 'image'");
        assert_eq!(filter_clause(ClipFilter::Files), " AND kind = 'files'");
        assert_eq!(filter_clause(ClipFilter::Favorites), " AND pinned = 1");
    }

    #[test]
    fn fts_filter_clauses_have_alias_prefix() {
        assert_eq!(filter_clause_fts(ClipFilter::All), "");
        assert_eq!(
            filter_clause_fts(ClipFilter::Text),
            " AND c.kind IN ('text','html','rtf')"
        );
        assert_eq!(
            filter_clause_fts(ClipFilter::Favorites),
            " AND c.pinned = 1"
        );
    }

    #[test]
    fn limit_is_clamped_to_range() {
        assert_eq!(clamp_limit(0), (1, true));
        assert_eq!(clamp_limit(50), (50, false));
        assert_eq!(clamp_limit(200), (200, false));
        assert_eq!(clamp_limit(500), (200, true));
    }
}
