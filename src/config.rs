use crate::fallback;
use crate::stage2;

/// Overridable settings loaded from `<data dir>/config.toml` (SPEC section 16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Shortest query stage 2 accepts, replacing `stage2::TYPO_MIN_QUERY_LEN`.
    pub typo_min_length: usize,
    /// Overrides for the disk fallback walk (SPEC section 11).
    pub fallback: FallbackSettings,
}

/// The `[fallback]` table of `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackSettings {
    /// Replaces `fallback::CHILD_DEPTH`.
    pub depth: usize,
    /// Replaces `fallback::ANCESTOR_LEVELS`.
    pub up: usize,
    /// ORed with the `--no-ignore` flag.
    pub no_ignore: bool,
    /// Replaces the built-in excluded directory names entirely.
    pub exclude: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            typo_min_length: stage2::TYPO_MIN_QUERY_LEN,
            fallback: FallbackSettings::default(),
        }
    }
}

impl Default for FallbackSettings {
    fn default() -> Self {
        FallbackSettings {
            depth: fallback::CHILD_DEPTH,
            up: fallback::ANCESTOR_LEVELS,
            no_ignore: false,
            exclude: fallback::EXCLUDED_NAMES
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
        }
    }
}

/// Parses `config.toml`'s text into `Settings`, filling defaults in for
/// every missing or invalid key and collecting one warning per problem.
pub fn parse(text: &str) -> (Settings, Vec<String>) {
    let mut settings = Settings::default();
    let mut warnings = Vec::new();
    let table = match text.parse::<toml::Table>() {
        Ok(table) => table,
        Err(_) => {
            warnings.push("config.toml is malformed TOML; using every default".to_owned());
            return (settings, warnings);
        }
    };
    for (key, value) in &table {
        match key.as_str() {
            "typo_min_length" => match value.as_integer().filter(|n| *n >= 1) {
                Some(length) => settings.typo_min_length = length as usize,
                None => warnings
                    .push("typo_min_length must be an integer >= 1; using the default".to_owned()),
            },
            "fallback" => parse_fallback(value, &mut settings.fallback, &mut warnings),
            "ambiguity" | "keyboard_layout" | "engine" => {
                warnings.push(format!("{key} is not supported yet; ignored"));
            }
            other => warnings.push(format!("unknown config key '{other}'; ignored")),
        }
    }
    (settings, warnings)
}

fn parse_fallback(
    value: &toml::Value,
    settings: &mut FallbackSettings,
    warnings: &mut Vec<String>,
) {
    let Some(table) = value.as_table() else {
        warnings.push("fallback must be a table; using its defaults".to_owned());
        return;
    };
    for (key, value) in table {
        match key.as_str() {
            "depth" => match value.as_integer().filter(|n| *n >= 1) {
                Some(depth) => settings.depth = depth as usize,
                None => warnings
                    .push("fallback.depth must be an integer >= 1; using the default".to_owned()),
            },
            "up" => match value.as_integer().filter(|n| *n >= 0) {
                Some(up) => settings.up = up as usize,
                None => warnings
                    .push("fallback.up must be an integer >= 0; using the default".to_owned()),
            },
            "no_ignore" => match value.as_bool() {
                Some(no_ignore) => settings.no_ignore = no_ignore,
                None => warnings
                    .push("fallback.no_ignore must be a boolean; using the default".to_owned()),
            },
            "exclude" => match parse_exclude(value) {
                Some(exclude) => settings.exclude = exclude,
                None => warnings.push(
                    "fallback.exclude must be an array of non-empty strings; using the default"
                        .to_owned(),
                ),
            },
            other => warnings.push(format!("unknown config key 'fallback.{other}'; ignored")),
        }
    }
}

fn parse_exclude(value: &toml::Value) -> Option<Vec<String>> {
    let array = value.as_array()?;
    let mut names = Vec::with_capacity(array.len());
    for item in array {
        let name = item.as_str()?;
        if name.is_empty() {
            return None;
        }
        names.push(name.to_owned());
    }
    Some(names)
}

#[cfg(test)]
mod tests {
    use super::{Settings, parse};

    #[test]
    fn empty_text_yields_every_default_and_no_warning() {
        let (settings, warnings) = parse("");
        assert_eq!(settings, Settings::default());
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_valid_typo_min_length_overrides_the_default() {
        let (settings, warnings) = parse("typo_min_length = 6");
        assert_eq!(settings.typo_min_length, 6);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_wrong_type_typo_min_length_warns_and_keeps_the_default() {
        let (settings, warnings) = parse("typo_min_length = \"six\"");
        assert_eq!(
            settings.typo_min_length,
            Settings::default().typo_min_length
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("typo_min_length"));
    }

    #[test]
    fn an_out_of_range_typo_min_length_warns_and_keeps_the_default() {
        let (settings, warnings) = parse("typo_min_length = 0");
        assert_eq!(
            settings.typo_min_length,
            Settings::default().typo_min_length
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("typo_min_length"));
    }

    #[test]
    fn a_valid_fallback_depth_overrides_the_default() {
        let (settings, warnings) = parse("[fallback]\ndepth = 3");
        assert_eq!(settings.fallback.depth, 3);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_wrong_type_fallback_depth_warns_and_keeps_the_default() {
        let (settings, warnings) = parse("[fallback]\ndepth = \"deep\"");
        assert_eq!(settings.fallback.depth, Settings::default().fallback.depth);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("fallback.depth"));
    }

    #[test]
    fn an_out_of_range_fallback_depth_warns_and_keeps_the_default() {
        let (settings, warnings) = parse("[fallback]\ndepth = 0");
        assert_eq!(settings.fallback.depth, Settings::default().fallback.depth);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("fallback.depth"));
    }

    #[test]
    fn a_valid_fallback_up_overrides_the_default() {
        let (settings, warnings) = parse("[fallback]\nup = 0");
        assert_eq!(settings.fallback.up, 0);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_wrong_type_fallback_up_warns_and_keeps_the_default() {
        let (settings, warnings) = parse("[fallback]\nup = \"one\"");
        assert_eq!(settings.fallback.up, Settings::default().fallback.up);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("fallback.up"));
    }

    #[test]
    fn an_out_of_range_fallback_up_warns_and_keeps_the_default() {
        let (settings, warnings) = parse("[fallback]\nup = -1");
        assert_eq!(settings.fallback.up, Settings::default().fallback.up);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("fallback.up"));
    }

    #[test]
    fn a_valid_fallback_no_ignore_overrides_the_default() {
        let (settings, warnings) = parse("[fallback]\nno_ignore = true");
        assert!(settings.fallback.no_ignore);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_wrong_type_fallback_no_ignore_warns_and_keeps_the_default() {
        let (settings, warnings) = parse("[fallback]\nno_ignore = \"yes\"");
        assert_eq!(
            settings.fallback.no_ignore,
            Settings::default().fallback.no_ignore
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("fallback.no_ignore"));
    }

    #[test]
    fn exclude_replaces_the_defaults_rather_than_extending_them() {
        let (settings, warnings) = parse("[fallback]\nexclude = [\"foo\", \"bar\"]");
        assert_eq!(
            settings.fallback.exclude,
            vec!["foo".to_owned(), "bar".to_owned()]
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_wrong_type_fallback_exclude_warns_and_keeps_the_default() {
        let (settings, warnings) = parse("[fallback]\nexclude = \"foo\"");
        assert_eq!(
            settings.fallback.exclude,
            Settings::default().fallback.exclude
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("fallback.exclude"));
    }

    #[test]
    fn a_fallback_exclude_with_an_empty_string_warns_and_keeps_the_default() {
        let (settings, warnings) = parse("[fallback]\nexclude = [\"\"]");
        assert_eq!(
            settings.fallback.exclude,
            Settings::default().fallback.exclude
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("fallback.exclude"));
    }

    #[test]
    fn an_unknown_top_level_key_warns_and_is_ignored() {
        let (settings, warnings) = parse("mystery = 1");
        assert_eq!(settings, Settings::default());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("mystery"));
    }

    #[test]
    fn an_unknown_fallback_key_warns_and_is_ignored() {
        let (settings, warnings) = parse("[fallback]\nmystery = 1");
        assert_eq!(settings.fallback, Settings::default().fallback);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("fallback.mystery"));
    }

    #[test]
    fn the_three_unsupported_keys_each_warn_not_supported_yet() {
        for key in ["ambiguity", "keyboard_layout", "engine"] {
            let (settings, warnings) = parse(&format!("{key} = 1"));
            assert_eq!(settings, Settings::default());
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].contains(key));
            assert!(warnings[0].contains("not supported yet"));
        }
    }

    #[test]
    fn malformed_toml_yields_every_default_and_one_warning() {
        let (settings, warnings) = parse("this is not [ toml");
        assert_eq!(settings, Settings::default());
        assert_eq!(warnings.len(), 1);
    }
}
