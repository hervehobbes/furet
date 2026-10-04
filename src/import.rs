use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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

/// The literal path argument of a `cd`-like PSReadLine history line, unquoted;
/// `None` when the line is not exactly one such command with one literal path.
pub fn parse_history_line(line: &str) -> Option<String> {
    if line.contains(['$', ';', '|', '`']) {
        return None;
    }
    let trimmed = line.trim();
    let (word, rest) = match trimmed.split_once(char::is_whitespace) {
        Some((word, rest)) => (word, rest.trim()),
        None => (trimmed, ""),
    };
    if !matches!(
        word.to_ascii_lowercase().as_str(),
        "cd" | "chdir" | "sl" | "set-location" | "pushd" | "push-location"
    ) {
        return None;
    }
    let rest = match rest.strip_prefix('-') {
        Some(after_dash) => {
            let (flag, tail) = after_dash.split_once(char::is_whitespace)?;
            match flag.to_ascii_lowercase().as_str() {
                "path" | "literalpath" => tail.trim_start(),
                _ => return None,
            }
        }
        None => rest,
    };
    if rest.is_empty() {
        return None;
    }
    let path_text = if rest.starts_with('\'') {
        if rest.len() < 2 || !rest.ends_with('\'') {
            return None;
        }
        unquote_single(&rest[1..rest.len() - 1])?
    } else if rest.starts_with('"') {
        if rest.len() < 2 || !rest.ends_with('"') {
            return None;
        }
        let inner = &rest[1..rest.len() - 1];
        if inner.contains('"') {
            return None;
        }
        inner.to_owned()
    } else {
        if rest.chars().any(char::is_whitespace) {
            return None;
        }
        rest.to_owned()
    };
    if path_text.is_empty() {
        return None;
    }
    Some(path_text)
}

fn unquote_single(inner: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\'' {
            if chars.next() != Some('\'') {
                return None;
            }
            out.push('\'');
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// `arg` with a leading `~` (alone, or followed by `\` or `/`) replaced by `home`.
pub fn expand_home(arg: &str, home: &Path) -> PathBuf {
    if arg == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = arg.strip_prefix("~\\").or_else(|| arg.strip_prefix("~/")) {
        return home.join(rest);
    }
    PathBuf::from(arg)
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

/// Collapses candidates sharing a `key` into the one with the highest
/// score (path ascending breaks a tie); returns it with the drop count.
pub fn dedupe_by_key(candidates: Vec<(f64, CanonicalDir)>) -> (Vec<(f64, CanonicalDir)>, usize) {
    let mut best: HashMap<String, (f64, CanonicalDir)> = HashMap::new();
    let mut duplicates = 0usize;
    for (score, dir) in candidates {
        match best.get(&dir.key) {
            Some((best_score, best_dir)) => {
                duplicates += 1;
                if score > *best_score || (score == *best_score && dir.path < best_dir.path) {
                    best.insert(dir.key.clone(), (score, dir));
                }
            }
            None => {
                best.insert(dir.key.clone(), (score, dir));
            }
        }
    }
    (best.into_values().collect(), duplicates)
}

/// Drops already-known directories, orders survivors by score descending
/// then path ascending, and assigns each a synthetic timestamp.
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
    use super::{Entry, dedupe_by_key, expand_home, parse_history_line, parse_line, plan};
    use crate::clock::Timestamp;
    use crate::paths::CanonicalDir;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

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
    fn case_variant_duplicates_collapse_to_the_higher_score() {
        let candidates = vec![(3.0, dir("c:\\dev\\Foo")), (9.0, dir("c:\\dev\\foo"))];
        let (deduped, duplicates) = dedupe_by_key(candidates);
        assert_eq!(duplicates, 1);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].0, 9.0);
        assert_eq!(deduped[0].1.path, "c:\\dev\\foo");

        let planned = plan(
            deduped,
            &HashSet::new(),
            Timestamp::from_unix_seconds(1_000),
        );
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].path, "c:\\dev\\foo");
    }

    #[test]
    fn duplicates_tied_on_score_keep_the_path_that_sorts_first() {
        let candidates = vec![(5.0, dir("c:\\dev\\Foo")), (5.0, dir("c:\\dev\\foo"))];
        let (deduped, duplicates) = dedupe_by_key(candidates);
        assert_eq!(duplicates, 1);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].1.path, "c:\\dev\\Foo");
    }

    #[test]
    fn the_best_score_wins_even_when_its_path_sorts_last() {
        let candidates = vec![(9.0, dir("c:\\dev\\fOO")), (3.0, dir("c:\\dev\\Foo"))];
        let (deduped, duplicates) = dedupe_by_key(candidates);
        assert_eq!(duplicates, 1);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].0, 9.0);
        assert_eq!(deduped[0].1.path, "c:\\dev\\fOO");
    }

    #[test]
    fn duplicates_with_identical_paths_keep_the_first_candidate() {
        let candidates = vec![
            (
                5.0,
                CanonicalDir {
                    path: "c:\\dev\\Foo".to_owned(),
                    key: "c:\\dev\\foo".to_owned(),
                    name: "first".to_owned(),
                    folder: None,
                },
            ),
            (
                5.0,
                CanonicalDir {
                    path: "c:\\dev\\Foo".to_owned(),
                    key: "c:\\dev\\foo".to_owned(),
                    name: "second".to_owned(),
                    folder: None,
                },
            ),
        ];
        let (deduped, duplicates) = dedupe_by_key(candidates);
        assert_eq!(duplicates, 1);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].1.name, "first");
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

    #[test]
    fn history_lines_with_a_cd_like_command_yield_their_path() {
        assert_eq!(parse_history_line("cd C:\\dev"), Some("C:\\dev".to_owned()));
        assert_eq!(
            parse_history_line("  CD   C:\\dev  "),
            Some("C:\\dev".to_owned())
        );
        for line in ["chdir C:\\dev", "sl C:\\dev", "Push-Location C:\\dev"] {
            assert_eq!(parse_history_line(line), Some("C:\\dev".to_owned()));
        }
        assert_eq!(
            parse_history_line("Set-Location -Path C:\\dev"),
            Some("C:\\dev".to_owned())
        );
        assert_eq!(
            parse_history_line("set-location -LITERALPATH C:\\dev"),
            Some("C:\\dev".to_owned())
        );
        assert_eq!(
            parse_history_line("pushd \"C:\\a b\""),
            Some("C:\\a b".to_owned())
        );
        assert_eq!(
            parse_history_line("sl -LiteralPath 'C:\\it''s'"),
            Some("C:\\it's".to_owned())
        );
        assert_eq!(parse_history_line("cd .."), Some("..".to_owned()));
        assert_eq!(parse_history_line("cd ~\\src"), Some("~\\src".to_owned()));
    }

    #[test]
    fn history_lines_that_are_not_one_literal_cd_are_ignored() {
        for line in [
            "ls C:\\dev",
            "cdx C:\\dev",
            "cd..",
            "cd",
            "cd -",
            "cd -Path",
            "cd -Path:C:\\dev",
            "pushd -StackName s C:\\dev",
            "cd $HOME",
            "cd C:\\dev; ls",
            "cd C:\\dev | Out-Null",
            "cd C:\\dev `",
            "cd C:\\Program Files",
            "cd 'C:\\dev",
            "cd 'a' 'b'",
            "cd \"a\"b\"",
            "cd ''",
            "cd '",
            "cd \"",
            "cd \"C:\\dev",
        ] {
            assert_eq!(parse_history_line(line), None, "line: {line:?}");
        }
    }

    #[test]
    fn expand_home_replaces_a_leading_tilde_only() {
        let home = Path::new("C:\\Users\\u");
        assert_eq!(expand_home("~", home), PathBuf::from("C:\\Users\\u"));
        assert_eq!(
            expand_home("~\\src", home),
            PathBuf::from("C:\\Users\\u\\src")
        );
        assert_eq!(
            expand_home("~/src", home),
            PathBuf::from("C:\\Users\\u\\src")
        );
        assert_eq!(expand_home("~x", home), PathBuf::from("~x"));
        assert_eq!(expand_home("C:\\dev", home), PathBuf::from("C:\\dev"));
    }
}
