use std::cmp::Ordering;
use std::collections::HashSet;

use crate::clock::Timestamp;
use crate::paths::CanonicalDir;

/// One parsed `zoxide query -ls` line: a score and its raw path text.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The zoxide score; only orders the import, never stored.
    pub score: f64,
    /// The path exactly as it appeared after the score, spaces included.
    pub path: String,
}

/// Parses one `<score> <path>` line; `None` for a missing score, a
/// non-numeric score, a missing path, or an empty line.
pub fn parse_line(line: &str) -> Option<Entry> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let score: f64 = parts.next()?.parse().ok()?;
    let path = parts.next()?.trim_start();
    if path.is_empty() {
        return None;
    }
    Some(Entry {
        score,
        path: path.to_owned(),
    })
}

/// One directory ready to import, with its synthetic recency timestamp.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    /// Canonical displayable path to record.
    pub path: String,
    /// The comparison key to store alongside it.
    pub key: String,
    /// Synthetic `ts`: strictly decreasing from `now - 1`, best score first.
    pub ts: Timestamp,
}

/// Drops already-known directories, orders survivors by score descending
/// then path ascending, and assigns each a synthetic timestamp counting
pub fn plan(
    mut candidates: Vec<(f64, CanonicalDir)>,
    known_keys: &HashSet<String>,
    now: Timestamp,
) -> Vec<Planned> {
    candidates.retain(|(_, dir)| !known_keys.contains(&dir.key));
    candidates.sort_by(|(score_a, dir_a), (score_b, dir_b)| {
        score_b
            .partial_cmp(score_a)
            .unwrap_or(Ordering::Equal)
            .then_with(|| dir_a.path.cmp(&dir_b.path))
    });
    candidates
        .into_iter()
        .enumerate()
        .map(|(index, (_, dir))| Planned {
            path: dir.path,
            key: dir.key,
            ts: Timestamp::from_unix_seconds(now.unix_seconds() - (index as i64 + 1)),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Entry, parse_line, plan};
    use crate::clock::Timestamp;
    use crate::paths::CanonicalDir;
    use std::collections::HashSet;

    fn dir(path: &str) -> CanonicalDir {
        CanonicalDir {
            path: path.to_owned(),
            key: path.to_lowercase(),
            name: String::new(),
            folder: None,
        }
    }

    #[test]
    fn leading_spaces_before_the_score_are_ignored() {
        assert_eq!(
            parse_line("  12.5 c:\\dev\\tokio"),
            Some(Entry {
                score: 12.5,
                path: "c:\\dev\\tokio".to_owned(),
            })
        );
    }

    #[test]
    fn a_path_containing_spaces_is_kept_whole() {
        assert_eq!(
            parse_line("3 c:\\dev\\my project"),
            Some(Entry {
                score: 3.0,
                path: "c:\\dev\\my project".to_owned(),
            })
        );
    }

    #[test]
    fn an_accented_path_round_trips() {
        assert_eq!(
            parse_line("1 c:\\dev\\r\u{e9}f\u{e9}rence"),
            Some(Entry {
                score: 1.0,
                path: "c:\\dev\\r\u{e9}f\u{e9}rence".to_owned(),
            })
        );
    }

    #[test]
    fn integer_and_decimal_scores_both_parse() {
        assert_eq!(
            parse_line("7 c:\\dev\\a").map(|entry| entry.score),
            Some(7.0)
        );
        assert_eq!(
            parse_line("7.25 c:\\dev\\a").map(|entry| entry.score),
            Some(7.25)
        );
    }

    #[test]
    fn a_line_without_a_score_is_malformed() {
        assert_eq!(parse_line("c:\\dev\\tokio"), None);
    }

    #[test]
    fn a_non_numeric_score_is_malformed() {
        assert_eq!(parse_line("abc c:\\dev\\tokio"), None);
    }

    #[test]
    fn a_score_with_no_path_is_malformed() {
        assert_eq!(parse_line("12.5"), None);
    }

    #[test]
    fn an_empty_line_is_malformed() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("   "), None);
    }

    #[test]
    fn plan_orders_by_score_descending_then_path_ascending() {
        let candidates = vec![
            (5.0, dir("c:\\dev\\b")),
            (5.0, dir("c:\\dev\\a")),
            (9.0, dir("c:\\dev\\z")),
        ];
        let planned = plan(
            candidates,
            &HashSet::new(),
            Timestamp::from_unix_seconds(1_000),
        );
        let paths: Vec<&str> = planned.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, ["c:\\dev\\z", "c:\\dev\\a", "c:\\dev\\b"]);
    }

    #[test]
    fn timestamps_strictly_decrease_from_now_minus_one() {
        let candidates = vec![
            (9.0, dir("c:\\dev\\z")),
            (5.0, dir("c:\\dev\\a")),
            (5.0, dir("c:\\dev\\b")),
        ];
        let now = Timestamp::from_unix_seconds(1_000);
        let planned = plan(candidates, &HashSet::new(), now);
        let seconds: Vec<i64> = planned
            .iter()
            .map(|entry| entry.ts.unix_seconds())
            .collect();
        assert_eq!(seconds, [999, 998, 997]);
    }

    #[test]
    fn known_keys_are_excluded_from_the_plan() {
        let candidates = vec![(9.0, dir("c:\\dev\\z")), (5.0, dir("c:\\dev\\a"))];
        let mut known = HashSet::new();
        known.insert("c:\\dev\\z".to_owned());
        let planned = plan(candidates, &known, Timestamp::from_unix_seconds(1_000));
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].path, "c:\\dev\\a");
    }
}
