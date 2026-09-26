use crate::fallback;
use crate::rank::Engine;
use crate::remove;
use crate::stage2;

/// Largest accepted `fallback.depth`; above it the parse warns and keeps the default.
pub const FALLBACK_MAX_DEPTH: usize = 5;
/// Largest accepted `fallback.up`; above it the parse warns and keeps the default.
pub const FALLBACK_MAX_UP: usize = 5;
/// Days of `visits` and `queries` history `furet add` keeps by default.
pub const RETENTION_DAYS: u32 = 365;

const ENGINE_WARNING: &str = r#"engine must be "reference" or "nucleo"; using "reference""#;

/// Overridable settings loaded from `<data dir>/config.toml` (SPEC section 16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Shortest query stage 2 accepts, replacing `stage2::TYPO_MIN_QUERY_LEN`.
    pub typo_min_length: usize,
    /// Overrides for the disk fallback walk (SPEC section 11).
    pub fallback: FallbackSettings,
    /// Absolute path `f` with no argument jumps to; `None` keeps the shell default.
    pub home: Option<String>,
    /// Days of `visits` and `queries` history kept; 0 keeps everything.
    pub retention_days: u32,
    /// Patterns of directories never recorded, read as `remove::Target`s.
    pub exclude_dirs: Vec<remove::Target>,
    /// Stage-1 scorer; `--engine` overrides it.
    pub engine: Engine,
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
            home: None,
            retention_days: RETENTION_DAYS,
            exclude_dirs: Vec::new(),
            engine: Engine::Reference,
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
            "home" => match value.as_str().filter(|text| !text.is_empty()) {
                Some(text) => settings.home = Some(text.to_owned()),
                None => {
                    warnings.push("home must be a non-empty string; using no override".to_owned())
                }
            },
            "retention_days" => match value.as_integer().and_then(|n| u32::try_from(n).ok()) {
                Some(days) => settings.retention_days = days,
                None => warnings
                    .push("retention_days must be an integer >= 0; using the default".to_owned()),
            },
            "exclude_dirs" => match parse_exclude(value) {
                Some(entries) => settings.exclude_dirs = exclusion_targets(&entries, &mut warnings),
                None => warnings.push(
                    "exclude_dirs must be an array of non-empty strings; using no exclusion"
                        .to_owned(),
                ),
            },
            "engine" => match value.as_str() {
                Some("reference") => settings.engine = Engine::Reference,
                Some("nucleo") => settings.engine = Engine::Nucleo,
                _ => warnings.push(ENGINE_WARNING.to_owned()),
            },
            "ambiguity" | "keyboard_layout" => {
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
            "depth" => match value
                .as_integer()
                .filter(|depth| (1..=FALLBACK_MAX_DEPTH as i64).contains(depth))
            {
                Some(depth) => settings.depth = depth as usize,
                None => warnings.push(format!(
                    "fallback.depth must be an integer in 1..={FALLBACK_MAX_DEPTH}; using the default"
                )),
            },
            "up" => match value
                .as_integer()
                .filter(|up| (0..=FALLBACK_MAX_UP as i64).contains(up))
            {
                Some(up) => settings.up = up as usize,
                None => warnings.push(format!(
                    "fallback.up must be an integer in 0..={FALLBACK_MAX_UP}; using the default"
                )),
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

fn exclusion_targets(entries: &[String], warnings: &mut Vec<String>) -> Vec<remove::Target> {
    let mut targets = Vec::with_capacity(entries.len());
    for entry in entries {
        match remove::exclusion_target(entry) {
            Some(target) => targets.push(target),
            None => warnings.push(format!(
                "exclude_dirs entry '{entry}' is a relative path; ignored"
            )),
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::{Settings, parse};
    use crate::rank::Engine;
    use crate::remove::Target;

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
    fn retention_days_defaults_to_one_year() {
        assert_eq!(Settings::default().retention_days, 365);
    }

    #[test]
    fn a_valid_retention_days_overrides_the_default() {
        let (settings, warnings) = parse("retention_days = 30");
        assert_eq!(settings.retention_days, 30);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_zero_retention_days_is_accepted_and_disables_the_purge() {
        let (settings, warnings) = parse("retention_days = 0");
        assert_eq!(settings.retention_days, 0);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_negative_or_wrong_type_retention_days_warns_and_keeps_the_default() {
        for text in ["retention_days = -1", "retention_days = \"year\""] {
            let (settings, warnings) = parse(text);
            assert_eq!(settings.retention_days, 365, "{text}");
            assert_eq!(warnings.len(), 1, "{text}");
            assert!(warnings[0].contains("retention_days"), "{text}");
        }
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
    fn a_fallback_depth_at_the_maximum_of_five_is_accepted() {
        let (settings, warnings) = parse("[fallback]\ndepth = 5");
        assert_eq!(settings.fallback.depth, 5);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_fallback_depth_above_the_maximum_warns_and_keeps_the_default() {
        let (settings, warnings) = parse("[fallback]\ndepth = 6");
        assert_eq!(settings.fallback.depth, Settings::default().fallback.depth);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("fallback.depth"));
    }

    #[test]
    fn a_huge_fallback_depth_warns_and_keeps_the_default() {
        let (settings, warnings) = parse("[fallback]\ndepth = 1000000");
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
    fn a_fallback_up_at_the_maximum_of_five_is_accepted() {
        let (settings, warnings) = parse("[fallback]\nup = 5");
        assert_eq!(settings.fallback.up, 5);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_fallback_up_above_the_maximum_warns_and_keeps_the_default() {
        let (settings, warnings) = parse("[fallback]\nup = 6");
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
    fn an_empty_exclude_list_disables_every_exclusion() {
        let (settings, warnings) = parse("[fallback]\nexclude = []");
        assert_eq!(settings.fallback.exclude, Vec::<String>::new());
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
    fn home_absent_yields_none() {
        let (settings, warnings) = parse("");
        assert_eq!(settings.home, None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_valid_home_string_overrides_the_default() {
        let (settings, warnings) = parse("home = \"C:/Users/dev\"");
        assert_eq!(settings.home.as_deref(), Some("C:/Users/dev"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_wrong_type_home_warns_and_keeps_none() {
        let (settings, warnings) = parse("home = 5");
        assert_eq!(settings.home, None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("home"));
    }

    #[test]
    fn an_empty_home_string_warns_and_keeps_none() {
        let (settings, warnings) = parse("home = \"\"");
        assert_eq!(settings.home, None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("home"));
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
    fn the_unsupported_keys_each_warn_not_supported_yet() {
        for key in ["ambiguity", "keyboard_layout"] {
            let (settings, warnings) = parse(&format!("{key} = 1"));
            assert_eq!(settings, Settings::default());
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].contains(key));
            assert!(warnings[0].contains("not supported yet"));
        }
    }

    #[test]
    fn engine_defaults_to_the_reference_engine() {
        assert_eq!(Settings::default().engine, Engine::Reference);
        assert_eq!(parse("").0.engine, Engine::Reference);
    }

    #[test]
    fn engine_nucleo_is_accepted() {
        let (settings, warnings) = parse("engine = \"nucleo\"");
        assert_eq!(settings.engine, Engine::Nucleo);
        assert!(warnings.is_empty());
        let (settings, warnings) = parse("engine = \"reference\"");
        assert_eq!(settings.engine, Engine::Reference);
        assert!(warnings.is_empty());
    }

    #[test]
    fn an_unknown_engine_name_warns_and_keeps_the_reference_engine() {
        let (settings, warnings) = parse("engine = \"fast\"");
        assert_eq!(settings.engine, Engine::Reference);
        assert_eq!(
            warnings,
            ["engine must be \"reference\" or \"nucleo\"; using \"reference\""]
        );
    }

    #[test]
    fn a_wrong_type_engine_warns_and_keeps_the_reference_engine() {
        let (settings, warnings) = parse("engine = 3");
        assert_eq!(settings.engine, Engine::Reference);
        assert_eq!(
            warnings,
            ["engine must be \"reference\" or \"nucleo\"; using \"reference\""]
        );
    }

    #[test]
    fn malformed_toml_yields_every_default_and_one_warning() {
        let (settings, warnings) = parse("this is not [ toml");
        assert_eq!(settings, Settings::default());
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn exclude_dirs_defaults_to_empty() {
        let (settings, warnings) = parse("");
        assert!(settings.exclude_dirs.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn exclude_dirs_parses_each_entry_through_exclusion_target() {
        let (settings, warnings) = parse("exclude_dirs = ['node_modules', 'C:\\Windows\\*']");
        assert_eq!(
            settings.exclude_dirs,
            vec![
                Target::Name("node_modules".to_owned()),
                Target::KeyPattern("c:\\windows\\*".to_owned()),
            ]
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_relative_exclude_dirs_entry_is_ignored_with_a_warning() {
        let (settings, warnings) = parse("exclude_dirs = ['a\\b', 'ok']");
        assert_eq!(settings.exclude_dirs, vec![Target::Name("ok".to_owned())]);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("a\\b"));
    }

    #[test]
    fn exclude_dirs_must_be_an_array_of_non_empty_strings() {
        for text in [
            "exclude_dirs = \"x\"",
            "exclude_dirs = [1]",
            "exclude_dirs = ['']",
        ] {
            let (settings, warnings) = parse(text);
            assert!(settings.exclude_dirs.is_empty(), "{text}");
            assert_eq!(warnings.len(), 1, "{text}");
            assert!(warnings[0].contains("exclude_dirs"), "{text}");
        }
    }
}
