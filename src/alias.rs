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

#[cfg(test)]
mod tests {
    use super::key;

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
}
