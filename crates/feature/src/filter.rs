//! Query parsing (`/alias` type filters) and combined type-filter + fuzzy rank.
//! Ported from the Go tool's `filter.go`, using `nucleo` for fuzzy matching.

use std::collections::BTreeMap;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Matcher, Utf32Str};

use crate::tracker::Issue;

/// A query line split into an explicit type filter and the remaining fuzzy text.
#[derive(Debug, Default, PartialEq)]
pub struct ParsedQuery {
    /// Empty = no type filter.
    pub active_type: String,
    pub search: String,
}

/// Interpret a leading `/<alias>` token against the alias map. `/b` or
/// `/b login` filters to the aliased type; an unknown `/x …` is literal text.
pub fn parse_query(raw: &str, aliases: &BTreeMap<String, String>) -> ParsedQuery {
    let Some(body) = raw.strip_prefix('/') else {
        return ParsedQuery {
            active_type: String::new(),
            search: raw.to_string(),
        };
    };
    let (token, rest, has_space) = match body.split_once(' ') {
        Some((t, r)) => (t, r, true),
        None => (body, "", false),
    };
    match aliases.get(token) {
        Some(type_name) => ParsedQuery {
            active_type: type_name.clone(),
            search: if has_space {
                rest.trim().to_string()
            } else {
                String::new()
            },
        },
        // Unknown alias — don't swallow the text, search literally.
        None => ParsedQuery {
            active_type: String::new(),
            search: raw.to_string(),
        },
    }
}

/// One filtered row: an index into the original issues, plus the char positions
/// within the summary that matched (for highlighting).
#[derive(Debug)]
pub struct RowMatch {
    pub issue_idx: usize,
    pub sum_matched: Vec<usize>,
}

/// Apply the type filter, then fuzzy-rank by the search text. With no search,
/// the type-filtered list keeps its original (server) order.
pub fn filter_issues(issues: &[Issue], q: &ParsedQuery, matcher: &mut Matcher) -> Vec<RowMatch> {
    let candidates: Vec<usize> = issues
        .iter()
        .enumerate()
        .filter(|(_, iss)| q.active_type.is_empty() || iss.kind == q.active_type)
        .map(|(i, _)| i)
        .collect();

    if q.search.is_empty() {
        return candidates
            .into_iter()
            .map(|issue_idx| RowMatch {
                issue_idx,
                sum_matched: Vec::new(),
            })
            .collect();
    }

    // Fuzzy-rank by "KEY summary"; map matched positions past "KEY " back to
    // summary-relative offsets.
    let pattern = Pattern::parse(&q.search, CaseMatching::Smart, Normalization::Smart);
    let mut scored: Vec<(u32, RowMatch)> = Vec::new();
    let mut buf = Vec::new();
    let mut indices = Vec::new();
    for &issue_idx in &candidates {
        let issue = &issues[issue_idx];
        let hay_str = format!("{} {}", issue.key, issue.summary);
        indices.clear();
        let hay = Utf32Str::new(&hay_str, &mut buf);
        if let Some(score) = pattern.indices(hay, matcher, &mut indices) {
            let base = issue.key.chars().count() as u32 + 1; // skip "KEY "
            let mut sum_matched: Vec<usize> = indices
                .iter()
                .filter(|&&p| p >= base)
                .map(|&p| (p - base) as usize)
                .collect();
            sum_matched.sort_unstable();
            sum_matched.dedup();
            scored.push((
                score,
                RowMatch {
                    issue_idx,
                    sum_matched,
                },
            ));
        }
    }
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, m)| m).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracker::Issue;
    use nucleo_matcher::Config;

    fn aliases() -> BTreeMap<String, String> {
        [
            ("b", "Bug"),
            ("bug", "Bug"),
            ("t", "Task"),
            ("s", "Story"),
            ("st", "Sub-task"),
            ("sub", "Sub-task"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    fn pq(active_type: &str, search: &str) -> ParsedQuery {
        ParsedQuery {
            active_type: active_type.to_string(),
            search: search.to_string(),
        }
    }

    #[test]
    fn parse_query_cases() {
        let a = aliases();
        assert_eq!(parse_query("login", &a), pq("", "login"));
        assert_eq!(parse_query("", &a), pq("", ""));
        assert_eq!(parse_query("/b", &a), pq("Bug", ""));
        assert_eq!(parse_query("/b ", &a), pq("Bug", ""));
        assert_eq!(parse_query("/b login", &a), pq("Bug", "login"));
        assert_eq!(parse_query("/sub thing", &a), pq("Sub-task", "thing"));
        assert_eq!(parse_query("/st x", &a), pq("Sub-task", "x"));
        assert_eq!(parse_query("/x foo", &a), pq("", "/x foo"));
        assert_eq!(parse_query("/ foo", &a), pq("", "/ foo"));
    }

    fn issue(key: &str, summary: &str, kind: &str) -> Issue {
        Issue {
            key: key.into(),
            summary: summary.into(),
            status: String::new(),
            assignee: String::new(),
            kind: kind.into(),
        }
    }

    fn sample() -> Vec<Issue> {
        vec![
            issue("DRM-1", "Fix login redirect", "Bug"),
            issue("DRM-2", "Add SSO login", "Task"),
            issue("DRM-3", "Login rate limit", "Story"),
            issue("DRM-4", "Unrelated cleanup", "Bug"),
        ]
    }

    fn idx_of(rows: &[RowMatch]) -> Vec<usize> {
        rows.iter().map(|r| r.issue_idx).collect()
    }

    #[test]
    fn type_filter_keeps_order() {
        let mut m = Matcher::new(Config::DEFAULT);
        let rows = filter_issues(&sample(), &pq("Bug", ""), &mut m);
        assert_eq!(idx_of(&rows), vec![0, 3]);
    }

    #[test]
    fn search_ranks_matches() {
        let mut m = Matcher::new(Config::DEFAULT);
        let rows = filter_issues(&sample(), &pq("", "login"), &mut m);
        let got = idx_of(&rows);
        assert_eq!(got.len(), 3);
        assert!(!got.contains(&3)); // "Unrelated cleanup" has no 'login' match
    }

    #[test]
    fn type_filter_plus_search() {
        let mut m = Matcher::new(Config::DEFAULT);
        let rows = filter_issues(&sample(), &pq("Bug", "login"), &mut m);
        assert_eq!(idx_of(&rows), vec![0]);
    }

    #[test]
    fn match_positions_map_into_summary_coords() {
        let mut m = Matcher::new(Config::DEFAULT);
        let rows = filter_issues(&sample(), &pq("Story", "login"), &mut m);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sum_matched, vec![0, 1, 2, 3, 4]);
    }
}
