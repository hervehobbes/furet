use serde::{Deserialize, Serialize};

/// The `format` marker every export carries.
pub const FORMAT: &str = "furet-export";

/// The format version this binary writes; lot 68 refuses others.
pub const VERSION: u32 = 1;

/// The whole database as one export document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub format: String,
    pub version: u32,
    pub furet: String,
    pub exported_at: i64,
    pub dirs: Vec<DirRecord>,
    pub visits: Vec<VisitRecord>,
    pub queries: Vec<QueryRecord>,
    pub aliases: Vec<AliasRecord>,
}

/// One `dirs` row; `missing_since` as stored, no reconcile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirRecord {
    pub path: String,
    pub key: String,
    pub first_seen: i64,
    pub missing_since: Option<i64>,
}

/// One `visits` row; `dir` and `from_dir` are the referenced `dirs` keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisitRecord {
    pub dir: String,
    pub ts: i64,
    pub source: String,
    pub session: String,
    pub from_dir: Option<String>,
}

/// One `queries` row; `result_dir` is the referenced `dirs` key, `null` when none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryRecord {
    pub ts: i64,
    pub cwd: String,
    pub query: String,
    pub result_dir: Option<String>,
    pub stage: String,
    pub outcome: String,
}

/// One `aliases` row, marks included.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AliasRecord {
    pub name: String,
    pub key: String,
    pub path: String,
    pub created: i64,
}

/// Serializes the snapshot as pretty JSON, with every non-ASCII character
/// escaped so the result is pure ASCII.
#[allow(clippy::expect_used)]
pub fn render(snapshot: &Snapshot) -> String {
    // WHY: serializing plain structs backed by String/i64 cannot fail; a panic here means a broken serde release.
    let json = serde_json::to_string_pretty(snapshot).expect("Snapshot serialization cannot fail");
    escape_non_ascii(&json)
}

/// Replaces every non-ASCII character with its UTF-16 units written as
/// `\u` plus 4 lowercase hex digits, keeping ASCII characters as is.
pub fn escape_non_ascii(json: &str) -> String {
    const HEX_DIGITS: [u8; 16] = *b"0123456789abcdef";
    let mut escaped = String::with_capacity(json.len());
    let mut units = [0u16; 2];
    for character in json.chars() {
        if character.is_ascii() {
            escaped.push(character);
            continue;
        }
        let written = character.encode_utf16(&mut units).len();
        for unit in &units[..written] {
            escaped.push_str("\\u");
            for shift in [12, 8, 4, 0] {
                let digit = ((*unit >> shift) & 0xF) as usize;
                escaped.push(HEX_DIGITS[digit] as char);
            }
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    // WHY: this is test code exercising the pure renderer.
    #![allow(clippy::expect_used)]

    use super::{
        AliasRecord, DirRecord, FORMAT, QueryRecord, Snapshot, VERSION, VisitRecord,
        escape_non_ascii, render,
    };

    #[test]
    fn escape_non_ascii_leaves_ascii_untouched_and_escapes_the_rest() {
        assert_eq!(escape_non_ascii("a\\b"), "a\\b");
        assert_eq!(escape_non_ascii("é"), "\\u00e9");
        assert_eq!(escape_non_ascii("😀"), "\\ud83d\\ude00");
        assert_eq!(
            escape_non_ascii("aé—b😀c"),
            "a\\u00e9\\u2014b\\ud83d\\ude00c"
        );
    }

    #[test]
    fn render_round_trips_through_serde_json() {
        let snapshot = Snapshot {
            format: FORMAT.to_owned(),
            version: VERSION,
            furet: "0.3.0".to_owned(),
            exported_at: 1_759_500_000,
            dirs: vec![
                DirRecord {
                    path: "C:\\dev\\café".to_owned(),
                    key: "c:\\dev\\café".to_owned(),
                    first_seen: 1_759_000_000,
                    missing_since: Some(1_759_100_000),
                },
                DirRecord {
                    path: "C:\\dev\\tokio".to_owned(),
                    key: "c:\\dev\\tokio".to_owned(),
                    first_seen: 1_759_000_100,
                    missing_since: None,
                },
            ],
            visits: vec![
                VisitRecord {
                    dir: "c:\\dev\\tokio".to_owned(),
                    ts: 1_759_000_200,
                    source: "jump".to_owned(),
                    session: "session-1".to_owned(),
                    from_dir: Some("c:\\dev\\café".to_owned()),
                },
                VisitRecord {
                    dir: "c:\\dev\\café".to_owned(),
                    ts: 1_759_000_300,
                    source: "hook".to_owned(),
                    session: "session-1".to_owned(),
                    from_dir: None,
                },
            ],
            queries: vec![
                QueryRecord {
                    ts: 1_759_000_400,
                    cwd: "C:\\dev".to_owned(),
                    query: "tok".to_owned(),
                    result_dir: Some("c:\\dev\\tokio".to_owned()),
                    stage: "1".to_owned(),
                    outcome: "jump".to_owned(),
                },
                QueryRecord {
                    ts: 1_759_000_500,
                    cwd: "C:\\dev".to_owned(),
                    query: "nope".to_owned(),
                    result_dir: None,
                    stage: "fallback".to_owned(),
                    outcome: "none".to_owned(),
                },
            ],
            aliases: vec![AliasRecord {
                name: "Ombi".to_owned(),
                key: "ombi".to_owned(),
                path: "C:\\dev\\café".to_owned(),
                created: 1_759_000_600,
            }],
        };
        let rendered = render(&snapshot);
        assert!(rendered.is_ascii(), "the rendered JSON must be pure ASCII");
        assert!(rendered.contains("\\u00e9"), "é is escaped: {rendered}");
        let parsed: Snapshot =
            serde_json::from_str(&rendered).expect("the rendered JSON parses back");
        assert_eq!(parsed, snapshot);
    }
}
