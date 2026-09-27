use std::path::{Component, Path};

use crate::paths;

/// Wildcard match of `pattern` against `text`, case-insensitively: `*` spans
/// any run of characters including `\`, `?` is exactly one, the rest is literal.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let mut pattern_index = 0usize;
    let mut text_index = 0usize;
    // WHY: one backtrack point keeps `*****x` linear instead of exponential.
    let mut last_star: Option<usize> = None;
    let mut star_mark = 0usize;
    while text_index < text.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == '?' || pattern[pattern_index] == text[text_index])
        {
            pattern_index += 1;
            text_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            last_star = Some(pattern_index);
            star_mark = text_index;
            pattern_index += 1;
        } else if let Some(star) = last_star {
            pattern_index = star + 1;
            star_mark += 1;
            text_index = star_mark;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

/// What a remove pattern resolves into: a directory name to match, an exact
/// lowercased path key, or a lowercased path wildcard pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The pattern carries no wildcard and no separator: it is matched
    /// against every normal segment, so a folder brings its descendants.
    Name(String),
    /// The pattern resolved to one directory: match its exact lowercased key.
    Key(String),
    /// The pattern is a path with wildcards: match lowercased paths against it.
    KeyPattern(String),
}

/// Whether `path` (a stored canonical path) is selected: a name target
/// matches any normal segment, the others the whole lowercased path.
pub fn matches(target: &Target, path: &str) -> bool {
    match target {
        Target::Name(pattern) => Path::new(path).components().any(|component| {
            let Component::Normal(segment) = component else {
                return false;
            };
            wildcard_match(pattern, &segment.to_string_lossy())
        }),
        Target::Key(key) => path.to_lowercase() == *key,
        Target::KeyPattern(pattern) => wildcard_match(pattern, &path.to_lowercase()),
    }
}

/// Whether the pattern is read as a path — it has `\`, `/` or `:`, or is
/// `.`/`..` — instead of a bare directory name.
pub fn is_path_pattern(pattern: &str) -> bool {
    pattern.contains('\\')
        || pattern.contains('/')
        || pattern.contains(':')
        || pattern == "."
        || pattern == ".."
}

/// The `exclude_dirs` counterpart of a remove pattern: pure (no cwd, no
/// disk), rejecting a relative path pattern instead of anchoring it.
pub fn exclusion_target(pattern: &str) -> Option<Target> {
    if !is_path_pattern(pattern) {
        return Some(Target::Name(pattern.to_owned()));
    }
    let unified = paths::unify_separators(pattern);
    if unified.starts_with('*') {
        return Some(Target::KeyPattern(unified.to_lowercase()));
    }
    if !Path::new(&unified).is_absolute() {
        return None;
    }
    if pattern.contains('*') || pattern.contains('?') {
        return Some(Target::KeyPattern(paths::absolute_key(
            pattern,
            Path::new(""),
        )));
    }
    Some(Target::Key(paths::absolute_key(pattern, Path::new(""))))
}

/// One parsed answer of the per-directory removal confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    All,
    Quit,
    Invalid,
}

/// Parses one stdin answer, trimmed and case-insensitive; `None` (EOF) is
/// `Quit`, an unrecognized line is `Invalid`.
pub fn parse_answer(line: Option<&str>) -> Answer {
    let Some(line) = line else {
        return Answer::Quit;
    };
    match line.trim().to_lowercase().as_str() {
        "y" | "yes" => Answer::Yes,
        "n" | "no" | "" => Answer::No,
        "a" | "all" => Answer::All,
        "q" | "quit" => Answer::Quit,
        _ => Answer::Invalid,
    }
}

/// Asks each question via `ask` (raw line, `None` = EOF); returns one `true`
/// per question meaning "remove": `All` decides the rest, `Quit` keeps it, `Invalid` re-asks.
pub fn confirm_each(
    questions: &[String],
    mut ask: impl FnMut(&str) -> Option<String>,
) -> Vec<bool> {
    let mut decisions = vec![false; questions.len()];
    let mut remove_rest = false;
    for (index, question) in questions.iter().enumerate() {
        if remove_rest {
            decisions[index] = true;
            continue;
        }
        loop {
            match parse_answer(ask(question).as_deref()) {
                Answer::Yes => {
                    decisions[index] = true;
                    break;
                }
                Answer::No => break,
                Answer::All => {
                    decisions[index] = true;
                    remove_rest = true;
                    break;
                }
                Answer::Quit => return decisions,
                Answer::Invalid => continue,
            }
        }
    }
    decisions
}

#[cfg(test)]
mod tests {
    use super::{
        Answer, Target, confirm_each, exclusion_target, is_path_pattern, matches, parse_answer,
        wildcard_match,
    };

    #[test]
    fn parse_answer_accepts_every_documented_form() {
        assert!(matches!(parse_answer(Some("y")), Answer::Yes));
        assert!(matches!(parse_answer(Some("YES")), Answer::Yes));
        assert!(matches!(parse_answer(Some(" n ")), Answer::No));
        assert!(matches!(parse_answer(Some("no")), Answer::No));
        assert!(matches!(parse_answer(Some("")), Answer::No));
        assert!(matches!(parse_answer(Some("a")), Answer::All));
        assert!(matches!(parse_answer(Some("All")), Answer::All));
        assert!(matches!(parse_answer(Some("q")), Answer::Quit));
        assert!(matches!(parse_answer(Some("quit")), Answer::Quit));
        assert!(matches!(parse_answer(None), Answer::Quit));
        assert!(matches!(parse_answer(Some("x")), Answer::Invalid));
        assert!(matches!(parse_answer(Some("yep")), Answer::Invalid));
    }

    #[test]
    fn confirm_each_applies_all_to_the_rest_without_asking() {
        let questions = ["q1", "q2", "q3", "q4"].map(String::from);
        let scripted = ["n".to_owned(), "a".to_owned()];
        let mut asked: Vec<String> = Vec::new();
        let mut feed = scripted.iter();
        let decisions = confirm_each(&questions, |question| {
            asked.push(question.to_owned());
            feed.next().cloned()
        });
        assert_eq!(decisions, [false, true, true, true]);
        assert_eq!(asked.len(), 2);
    }

    #[test]
    fn confirm_each_quit_keeps_the_rest_and_keeps_earlier_yes() {
        let questions = ["q1", "q2", "q3"].map(String::from);
        let scripted = ["y".to_owned(), "q".to_owned()];
        let mut asked: Vec<String> = Vec::new();
        let mut feed = scripted.iter();
        let decisions = confirm_each(&questions, |question| {
            asked.push(question.to_owned());
            feed.next().cloned()
        });
        assert_eq!(decisions, [true, false, false]);
        assert_eq!(asked.len(), 2);
    }

    #[test]
    fn confirm_each_reasks_the_same_question_after_an_invalid_answer() {
        let questions = ["q1".to_owned()];
        let scripted = ["x".to_owned(), "y".to_owned()];
        let mut asked: Vec<String> = Vec::new();
        let mut feed = scripted.iter();
        let decisions = confirm_each(&questions, |question| {
            asked.push(question.to_owned());
            feed.next().cloned()
        });
        assert_eq!(decisions, [true]);
        assert_eq!(asked, ["q1".to_owned(), "q1".to_owned()]);
    }

    #[test]
    fn confirm_each_treats_eof_as_quit() {
        let questions = ["q1", "q2", "q3"].map(String::from);
        let mut calls = 0usize;
        let decisions = confirm_each(&questions, |_| {
            calls += 1;
            None
        });
        assert_eq!(decisions, [false, false, false]);
        assert_eq!(calls, 1);
    }

    #[test]
    fn a_star_matches_any_suffix_including_none() {
        assert!(wildcard_match("ombi*", "ombi"));
        assert!(wildcard_match("ombi*", "ombi-v4"));
        assert!(!wildcard_match("ombi*", "xombi"));
    }

    #[test]
    fn a_question_mark_matches_exactly_one_character() {
        assert!(wildcard_match("ombi-?", "ombi-v"));
        assert!(!wildcard_match("ombi-?", "ombi-v4"));
        assert!(!wildcard_match("ombi-?", "ombi-"));
    }

    #[test]
    fn matching_ignores_case() {
        assert!(wildcard_match("OMBI", "ombi"));
        assert!(wildcard_match("ombi", "Ombi"));
        assert!(matches(
            &Target::Key("c:\\apps\\ombi".to_owned()),
            "C:\\Apps\\Ombi"
        ));
    }

    #[test]
    fn brackets_and_dots_are_literal() {
        assert!(wildcard_match("[a]pp.json", "[a]pp.json"));
        assert!(!wildcard_match("[a]pp.json", "app.json"));
        assert!(!wildcard_match("[a]pp.json", "bpp.json"));
        assert!(wildcard_match("ombi.", "ombi."));
        assert!(!wildcard_match("ombi.", "ombix"));
    }

    #[test]
    fn a_star_crosses_separators() {
        assert!(wildcard_match("c:\\apps\\*", "c:\\apps\\a\\b"));
        assert!(!wildcard_match("c:\\apps\\*", "c:\\apps"));
    }

    #[test]
    fn many_stars_still_match_in_linear_time() {
        let pattern = "*".repeat(30) + "x";
        let text = "y".repeat(2_000);
        assert!(!wildcard_match(&pattern, &text));
        assert!(wildcard_match(
            &(pattern.clone() + "*"),
            &format!("{text}x")
        ));
    }

    #[test]
    fn a_separator_a_colon_or_a_dot_form_selects_path_mode() {
        for pattern in ["a\\b", "a/b", "c:", ".", ".."] {
            assert!(is_path_pattern(pattern), "{pattern} must select path mode");
        }
        for pattern in ["ombi*", ".git"] {
            assert!(!is_path_pattern(pattern), "{pattern} must select name mode");
        }
    }

    #[test]
    fn name_targets_match_any_normal_segment() {
        let appdata = Target::Name("*appdata*".to_owned());
        assert!(matches(&appdata, "C:\\Users\\x\\AppData\\Local\\y"));
        let prefix = Target::Name("ombi*".to_owned());
        assert!(matches(&prefix, "C:\\apps\\ombi"));
        assert!(matches(&prefix, "C:\\apps\\ombi\\sub"));
        assert!(!matches(&prefix, "C:\\apps\\xombi"));
    }

    #[test]
    fn a_name_pattern_never_matches_the_drive_prefix() {
        let drive = Target::Name("c*".to_owned());
        assert!(!matches(&drive, "C:\\dev\\x"));
        assert!(matches(&drive, "C:\\dev\\cache"));
    }

    #[test]
    fn exclusion_target_keeps_names_absolute_paths_and_star_patterns() {
        assert_eq!(
            exclusion_target("node_modules"),
            Some(Target::Name("node_modules".to_owned()))
        );
        assert_eq!(
            exclusion_target("C:\\Windows\\*"),
            Some(Target::KeyPattern("c:\\windows\\*".to_owned()))
        );
        assert_eq!(
            exclusion_target("C:/Temp/Build/"),
            Some(Target::Key("c:\\temp\\build".to_owned()))
        );
        assert_eq!(
            exclusion_target("*\\target\\*"),
            Some(Target::KeyPattern("*\\target\\*".to_owned()))
        );
    }

    #[test]
    fn exclusion_target_rejects_relative_paths() {
        for pattern in ["a\\b", "a/b", ".", "..", "\\x", "C:x"] {
            assert_eq!(
                exclusion_target(pattern),
                None,
                "{pattern} must be rejected"
            );
        }
    }
}
