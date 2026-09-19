use std::fs;
use std::path::Path;

use serde::Deserialize;

#[derive(Deserialize)]
struct ScenarioFile {
    case: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    query: String,
    #[serde(default)]
    dirs: Vec<String>,
    #[serde(default)]
    jump: Option<String>,
    #[serde(default)]
    menu: Option<Vec<String>>,
    #[serde(default)]
    none: Option<bool>,
}

#[test]
fn scenario_files_parse_with_the_expected_shape() -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scenarios");
    let mut files_seen = 0usize;
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        let is_toml = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"));
        if !is_toml {
            continue;
        }
        files_seen += 1;
        let text = fs::read_to_string(&path)?;
        let parsed: ScenarioFile =
            toml::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))?;
        assert!(
            !parsed.case.is_empty(),
            "{}: expected at least one [[case]] table",
            path.display()
        );
        for case in &parsed.case {
            assert!(
                !case.name.trim().is_empty(),
                "{}: every case needs a non-empty name",
                path.display()
            );
            assert!(
                !case.query.trim().is_empty(),
                "{}: case '{}' needs a non-empty query",
                path.display(),
                case.name
            );
            assert!(
                case.dirs.iter().all(|d| !d.trim().is_empty()),
                "{}: case '{}' has an empty entry in dirs",
                path.display(),
                case.name
            );
            let outcomes = [
                case.jump.is_some(),
                case.menu.is_some(),
                case.none.is_some(),
            ];
            let chosen = outcomes.iter().filter(|set| **set).count();
            assert_eq!(
                chosen,
                1,
                "{}: case '{}' must set exactly one of jump / menu / none",
                path.display(),
                case.name
            );
            assert!(
                case.none.unwrap_or(true),
                "{}: case '{}' sets none = false; drop the key or use a real outcome",
                path.display(),
                case.name
            );
        }
    }
    assert!(
        files_seen > 0,
        "no .toml files found under {}",
        dir.display()
    );
    Ok(())
}

#[test]
fn expect_remains_usable_in_test_code() {
    // WHY: guards the allow-expect-in-tests exemption the clippy setup relies on.
    let digits = [7u8, 13];
    assert_eq!(digits.first().expect("array literal is not empty"), &7);
}
