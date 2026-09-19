use std::fs;
use std::path::{Path, PathBuf};

use furet::clock::{Clock, FixedClock, Timestamp};
use furet::decision::{Decision, decide};
use furet::rank::{Candidate, rank};
use serde::Deserialize;

const SIMULATED_NOW: Timestamp = Timestamp::from_unix_seconds(1_700_000_000);

#[derive(Deserialize)]
struct ScenarioFile {
    case: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    query: String,
    #[serde(default)]
    dirs: Vec<DirSpec>,
    #[serde(default)]
    current_dir: Option<String>,
    #[serde(default)]
    jump: Option<String>,
    #[serde(default)]
    menu: Option<Vec<String>>,
    #[serde(default)]
    none: Option<bool>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DirSpec {
    Plain(String),
    Detailed {
        path: String,
        #[serde(default)]
        visited: Option<String>,
        #[serde(default)]
        missing: bool,
    },
}

impl DirSpec {
    fn path(&self) -> &str {
        match self {
            DirSpec::Plain(path) => path,
            DirSpec::Detailed { path, .. } => path,
        }
    }

    fn visited(&self) -> Option<&str> {
        match self {
            DirSpec::Plain(_) => None,
            DirSpec::Detailed { visited, .. } => visited.as_deref(),
        }
    }

    fn missing(&self) -> bool {
        match self {
            DirSpec::Plain(_) => false,
            DirSpec::Detailed { missing, .. } => *missing,
        }
    }
}

fn scenario_files() -> Result<Vec<(PathBuf, ScenarioFile)>, Box<dyn std::error::Error>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scenarios");
    let mut loaded = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        let is_toml = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"));
        if !is_toml {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let parsed: ScenarioFile =
            toml::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))?;
        loaded.push((path, parsed));
    }
    assert!(
        !loaded.is_empty(),
        "no .toml files found under {}",
        dir.display()
    );
    Ok(loaded)
}

fn seconds_before_now(offset: &str) -> i64 {
    let rest = offset
        .strip_prefix('-')
        .unwrap_or_else(|| panic!("a visit offset starts with '-': '{offset}'"));
    let unit = rest
        .chars()
        .last()
        .unwrap_or_else(|| panic!("a visit offset needs a unit suffix: '{offset}'"));
    let digits = &rest[..rest.len() - unit.len_utf8()];
    let count: i64 = digits
        .parse()
        .unwrap_or_else(|_| panic!("a visit offset reads -<number><unit>: '{offset}'"));
    let scale = match unit {
        'h' => 3_600,
        'd' => 86_400,
        _ => panic!("unknown visit offset unit '{unit}' in '{offset}'"),
    };
    count * scale
}

fn split_path(path: &str) -> (&str, Option<&str>) {
    match path.rfind(['/', '\\']) {
        Some(index) => (
            &path[index + 1..],
            Some(&path[..index]).filter(|folder| !folder.is_empty()),
        ),
        None => (path, None),
    }
}

fn candidate(spec: &DirSpec, now: Timestamp) -> Candidate {
    let path = spec.path();
    let (name, folder) = split_path(path);
    let offset = spec.visited().map_or(0, seconds_before_now);
    Candidate {
        path: path.to_owned(),
        name: name.to_owned(),
        folder: folder.map(str::to_owned),
        last_visit: Timestamp::from_unix_seconds(now.unix_seconds() - offset),
        missing: spec.missing(),
    }
}

#[test]
fn scenario_files_parse_with_the_expected_shape() -> Result<(), Box<dyn std::error::Error>> {
    for (path, parsed) in scenario_files()? {
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
                case.dirs.iter().all(|dir| !dir.path().trim().is_empty()),
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
    Ok(())
}

#[test]
fn scenario_cases_reach_their_expected_outcome() -> Result<(), Box<dyn std::error::Error>> {
    let clock = FixedClock::new(SIMULATED_NOW);
    let mut cases_run = 0usize;
    for (path, parsed) in scenario_files()? {
        for case in &parsed.case {
            let candidates: Vec<Candidate> = case
                .dirs
                .iter()
                .map(|spec| candidate(spec, clock.now()))
                .collect();
            let current_dir = case.current_dir.as_deref().unwrap_or_default();
            let ranked = rank(&case.query, current_dir, &candidates);
            let found: Vec<&str> = ranked
                .iter()
                .map(|scored| scored.candidate.path.as_str())
                .collect();
            let decision = decide(&ranked);
            match (&case.jump, &case.menu, case.none) {
                (Some(expected), None, None) => match &decision {
                    Decision::Jump(candidate) => assert_eq!(
                        candidate.path,
                        *expected,
                        "{}: case '{}' ranked {found:?}",
                        path.display(),
                        case.name
                    ),
                    other => panic!(
                        "{}: case '{}' expected a jump to '{expected}', decided {other:?}",
                        path.display(),
                        case.name
                    ),
                },
                (None, Some(expected), None) => match &decision {
                    Decision::Menu(shown) => {
                        let listed: Vec<&str> = shown
                            .iter()
                            .map(|candidate| candidate.path.as_str())
                            .collect();
                        assert_eq!(
                            listed,
                            expected.iter().map(String::as_str).collect::<Vec<&str>>(),
                            "{}: case '{}' ranked {found:?}",
                            path.display(),
                            case.name
                        );
                    }
                    other => panic!(
                        "{}: case '{}' expected a menu of {expected:?}, decided {other:?}",
                        path.display(),
                        case.name
                    ),
                },
                (None, None, Some(_)) => assert!(
                    matches!(decision, Decision::None),
                    "{}: case '{}' expected no candidate, ranked {found:?}",
                    path.display(),
                    case.name
                ),
                _ => panic!(
                    "{}: case '{}' must set exactly one of jump / menu / none",
                    path.display(),
                    case.name
                ),
            }
            cases_run += 1;
        }
    }
    assert!(cases_run > 0, "no scenario case was executed");
    Ok(())
}

#[test]
fn a_relative_visit_offset_resolves_against_the_simulated_clock() {
    let clock = FixedClock::new(SIMULATED_NOW);
    assert_eq!(seconds_before_now("-2h"), 7_200);
    assert_eq!(seconds_before_now("-3d"), 259_200);
    assert_eq!(seconds_before_now("-0h"), 0);
    let spec = DirSpec::Detailed {
        path: "/dev/tokio".to_owned(),
        visited: Some("-2h".to_owned()),
        missing: false,
    };
    let built = candidate(&spec, clock.now());
    assert_eq!(built.name, "tokio");
    assert_eq!(built.folder.as_deref(), Some("/dev"));
    assert_eq!(
        built.last_visit,
        Timestamp::from_unix_seconds(SIMULATED_NOW.unix_seconds() - 7_200)
    );
    assert!(!built.missing);
}

#[test]
fn a_plain_dir_entry_is_present_and_visited_now() {
    let clock = FixedClock::new(SIMULATED_NOW);
    let built = candidate(&DirSpec::Plain("/sandbox/furet".to_owned()), clock.now());
    assert_eq!(built.path, "/sandbox/furet");
    assert_eq!(built.last_visit, SIMULATED_NOW);
    assert!(!built.missing);
    assert_eq!(split_path("furet"), ("furet", None));
    assert_eq!(split_path("/furet"), ("furet", None));
    assert_eq!(split_path("c:\\dev\\furet"), ("furet", Some("c:\\dev")));
}

#[test]
fn expect_remains_usable_in_test_code() {
    // WHY: guards the allow-expect-in-tests exemption the clippy setup relies on.
    let digits = [7u8, 13];
    assert_eq!(digits.first().expect("array literal is not empty"), &7);
}
