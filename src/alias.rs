use crate::stage2;

/// The lookup key of an alias name: lowercased when every char is ASCII
/// alphanumeric, `_` or `-`; `None` when the name is empty or has another char.
pub fn key(name: &str) -> Option<String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    Some(name.to_ascii_lowercase())
}

/// The known name at distance exactly 1 from `key` (optimal string alignment
/// on lowercased chars); on several, the one with the smallest key.
pub fn suggestion<'a>(key: &str, names: &'a [String]) -> Option<&'a str> {
    let query: Vec<char> = key.chars().collect();
    let mut best: Option<(&'a str, String)> = None;
    for name in names {
        let lower = name.to_ascii_lowercase();
        let candidate: Vec<char> = lower.chars().collect();
        if stage2::optimal_string_alignment(&query, &candidate) != 1 {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(_, best_lower)| lower < *best_lower)
        {
            best = Some((name.as_str(), lower));
        }
    }
    best.map(|(name, _)| name)
}

/// The mark digit `1`-`9` that `name` denotes, if it is one.
pub fn mark_digit(name: &str) -> Option<u8> {
    let mut chars = name.chars();
    let digit = chars.next()?;
    if chars.next().is_some() || !matches!(digit, '1'..='9') {
        return None;
    }
    Some(digit as u8 - b'0')
}

/// The marks a `furet mark delete` spec covers: one digit or an ascending
/// range `a-b`, both ends `1`-`9`.
pub fn mark_range(spec: &str) -> Option<std::ops::RangeInclusive<u8>> {
    let (start, end) = match spec.split_once('-') {
        None => (spec, spec),
        Some(("", _)) | Some((_, "")) => return None,
        Some((start, end)) => (start, end),
    };
    let start = mark_digit(start)?;
    let end = mark_digit(end)?;
    (start <= end).then_some(start..=end)
}

#[cfg(test)]
mod tests {
    use super::{key, mark_digit, mark_range, suggestion};

    #[test]
    fn key_lowercases_valid_names_ignoring_case() {
        assert_eq!(key("ombi").as_deref(), Some("ombi"));
        assert_eq!(key("Ombi").as_deref(), Some("ombi"));
        assert_eq!(key("1").as_deref(), Some("1"));
        assert_eq!(key("a_b-C9").as_deref(), Some("a_b-c9"));
    }

    #[test]
    fn key_rejects_empty_names_and_any_other_character() {
        for name in ["", "!ombi", "=ombi", "om bi", "a/b", "a\\b", "a.b", "é"] {
            assert_eq!(key(name), None, "name '{name}' must be invalid");
        }
    }

    #[test]
    fn suggestion_hints_at_one_edit_names_ignoring_case() {
        let ombi = ["Ombi".to_owned()];
        assert_eq!(suggestion("mbi", &ombi), Some("Ombi"));
        assert_eq!(suggestion("obmi", &["ombi".to_owned()]), Some("ombi"));
    }

    #[test]
    fn suggestion_stays_silent_at_distance_two_or_beyond_an_empty_pool() {
        assert_eq!(suggestion("zzzz", &["ombi".to_owned()]), None);
        assert_eq!(suggestion("ab", &[]), None);
    }

    #[test]
    fn suggestion_breaks_a_tie_on_the_smallest_lowercase_name() {
        let names = ["xb".to_owned(), "ac".to_owned()];
        assert_eq!(suggestion("ab", &names), Some("ac"));
    }

    #[test]
    fn mark_digit_accepts_only_one_char_from_one_to_nine() {
        assert_eq!(mark_digit("1"), Some(1));
        assert_eq!(mark_digit("9"), Some(9));
        for name in ["0", "10", "a", ""] {
            assert_eq!(mark_digit(name), None, "name '{name}' is not a mark");
        }
    }

    #[test]
    fn mark_range_accepts_a_digit_and_an_ascending_range() {
        assert_eq!(mark_range("3"), Some(3..=3));
        assert_eq!(mark_range("2-4"), Some(2..=4));
        assert_eq!(mark_range("5-5"), Some(5..=5));
    }

    #[test]
    fn mark_range_rejects_every_invalid_spec() {
        for spec in ["0", "10", "4-2", "2-", "-2", "2-10", "a", "", "2 - 4"] {
            assert_eq!(mark_range(spec), None, "spec '{spec}' is not a mark range");
        }
    }
}
