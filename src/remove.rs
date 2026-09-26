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
    /// The pattern carries no wildcard and no separator: match directory names.
    Name(String),
    /// The pattern resolved to one directory: match its exact lowercased key.
    Key(String),
    /// The pattern is a path with wildcards: match lowercased paths against it.
    KeyPattern(String),
}

/// Whether `path` (a stored canonical path) is selected by `target`.
pub fn matches(target: &Target, path: &str) -> bool {
    match target {
        Target::Name(pattern) => wildcard_match(pattern, &paths::split(path).name),
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

#[cfg(test)]
mod tests {
    use super::{Target, is_path_pattern, matches, wildcard_match};

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
    fn name_targets_compare_the_last_segment_only() {
        let name = Target::Name("src".to_owned());
        assert!(matches(&name, "c:\\dev\\proj\\src"));
        assert!(matches(&name, "c:\\dev\\proj\\Src"));
        assert!(!matches(&name, "c:\\dev\\proj\\src\\bin"));
        assert!(!matches(&name, "c:\\dev\\src\\bin"));
    }
}
