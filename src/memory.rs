use crate::normalize::Normalized;
use crate::rank::{Scored, same_path};

/// What the query journal remembers for one query (SPEC-v2 §24), as looked
/// up by the outer layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recall {
    /// `query_memory = false`: no lookup happened at all.
    Disabled,
    /// No `jump` or `pick` row with a result shares the query's memory key.
    Nothing,
    /// The latest such row is a probable failure, which cancels the memory.
    ProbableFailure,
    /// The latest such row's directory, with the local date it was chosen.
    Remembered { path: String, chosen: String },
}

impl Recall {
    /// The remembered directory's path, when there is one.
    pub fn path(&self) -> Option<&str> {
        match self {
            Recall::Remembered { path, .. } => Some(path),
            _ => None,
        }
    }
}

/// The memory key of a query: each whitespace-separated token normalized
/// with SPEC §7.1, joined by one space.
pub fn key(query: &str) -> String {
    query
        .split_whitespace()
        .map(|token| Normalized::new(token).text())
        .filter(|token| !token.is_empty())
        .collect::<Vec<String>>()
        .join(" ")
}

/// Same answer as `key(text) == key`, without allocating when `text` is
/// ASCII, whose normalization is only lowercasing.
pub fn matches(text: &str, key: &str) -> bool {
    if !text.is_ascii() {
        return self::key(text) == key;
    }
    let mut expected = key.split(' ').filter(|wanted| !wanted.is_empty());
    let tokens_match = text.split_whitespace().all(|token| {
        expected.next().is_some_and(|wanted| {
            token.len() == wanted.len()
                && token
                    .bytes()
                    .zip(wanted.bytes())
                    .all(|(have, want)| have.to_ascii_lowercase() == want)
        })
    });
    tokens_match && expected.next().is_none()
}

/// True when `remembered` is one of the ranked candidates.
pub fn applies(ranked: &[Scored], remembered: Option<&str>) -> bool {
    remembered.is_some_and(|path| {
        ranked
            .iter()
            .any(|scored| same_path(&scored.candidate.path, path))
    })
}

/// Moves the remembered directory, when ranked, to the front; every other
/// candidate keeps its relative order.
pub fn promote<'a>(mut ranked: Vec<Scored<'a>>, remembered: Option<&str>) -> Vec<Scored<'a>> {
    let position = remembered.and_then(|path| {
        ranked
            .iter()
            .position(|scored| same_path(&scored.candidate.path, path))
    });
    if let Some(index) = position {
        let head = ranked.remove(index);
        ranked.insert(0, head);
    }
    ranked
}

#[cfg(test)]
mod tests {
    use super::{Recall, applies, key, matches, promote};
    use crate::clock::Timestamp;
    use crate::rank::{Candidate, Engine, Scored, rank};
    use crate::stage2::TYPO_MIN_QUERY_LEN;
    use proptest::prelude::*;
    use std::collections::HashSet;

    static NAMES: &[&str] = &["tokio", "tokei", "helix", "ombi", "om", "Réunions"];
    static FOLDERS: &[&str] = &["/dev", "/DEV", "/archive"];
    static QUERIES: &[&str] = &["om", "tok", "tokio", "tokoi", "helix", "reunions", "zigzag"];
    static CURRENT: &[&str] = &["", "/dev/tokio", "/DEV/OM", "/archive/helix"];

    fn at(hours: i64) -> Timestamp {
        Timestamp::from_unix_seconds(1_700_000_000 - hours * 3_600)
    }

    fn a_candidate_set() -> impl Strategy<Value = Vec<Candidate>> {
        proptest::collection::vec(
            (
                proptest::sample::select(FOLDERS),
                proptest::sample::select(NAMES),
                0i64..48,
                any::<bool>(),
            ),
            0..10,
        )
        .prop_map(|drawn| {
            let mut seen: HashSet<String> = HashSet::new();
            drawn
                .into_iter()
                .map(|(folder, name, hours, missing)| Candidate {
                    path: format!("{folder}/{name}"),
                    name: name.to_owned(),
                    folder: Some(folder.to_owned()),
                    last_visit: at(hours),
                    missing,
                })
                .filter(|candidate| seen.insert(candidate.path.to_lowercase()))
                .collect()
        })
    }

    fn ranked<'a>(query: &str, current: &str, candidates: &'a [Candidate]) -> Vec<Scored<'a>> {
        rank(
            query,
            current,
            candidates,
            TYPO_MIN_QUERY_LEN,
            Engine::Reference,
        )
    }

    fn paths<'a>(scored: &[Scored<'a>]) -> Vec<&'a str> {
        scored
            .iter()
            .map(|entry| entry.candidate.path.as_str())
            .collect()
    }

    fn cased(path: &str, upper: bool) -> String {
        if upper {
            path.to_uppercase()
        } else {
            path.to_owned()
        }
    }

    #[test]
    fn a_query_differing_only_in_case_accents_or_spaces_shares_its_key() {
        let expected = key("tok io");
        for variant in ["Tok io", "  tok   io ", "TOK\tIO", "tök ío", "tok io\n"] {
            assert_eq!(key(variant), expected, "variant {variant:?}");
        }
        assert_eq!(expected, "tok io");
        assert_eq!(key("Réunions"), "reunions");
        assert_eq!(key("   "), "");
    }

    #[test]
    fn a_different_token_order_gives_a_different_key() {
        assert_ne!(key("tok io"), key("io tok"));
        assert_ne!(key("tokio"), key("tok io"));
    }

    #[test]
    fn the_better_match_loses_to_the_remembered_directory() {
        let candidates = [
            Candidate {
                path: "c:\\om".to_owned(),
                name: "om".to_owned(),
                folder: Some("c:".to_owned()),
                last_visit: at(1),
                missing: false,
            },
            Candidate {
                path: "c:\\dev\\ombi".to_owned(),
                name: "ombi".to_owned(),
                folder: Some("c:\\dev".to_owned()),
                last_visit: at(2),
                missing: false,
            },
        ];
        let normal = ranked("om", "", &candidates);
        assert_eq!(paths(&normal), ["c:\\om", "c:\\dev\\ombi"]);
        assert!(applies(&normal, Some("C:\\Dev\\Ombi")));
        let promoted = promote(normal, Some("C:\\Dev\\Ombi"));
        assert_eq!(paths(&promoted), ["c:\\dev\\ombi", "c:\\om"]);
    }

    #[test]
    fn a_recall_exposes_a_path_only_when_it_remembers_one() {
        let remembered = Recall::Remembered {
            path: "c:\\dev\\ombi".to_owned(),
            chosen: "2026-09-27T10:00:00".to_owned(),
        };
        assert_eq!(remembered.path(), Some("c:\\dev\\ombi"));
        assert_eq!(Recall::Disabled.path(), None);
        assert_eq!(Recall::Nothing.path(), None);
        assert_eq!(Recall::ProbableFailure.path(), None);
    }

    #[test]
    fn matches_compares_a_journaled_text_with_a_key() {
        let wanted = key("tok io");
        for text in ["tok io", "TOK  Io", "\ttok io ", "tök ío"] {
            assert!(matches(text, &wanted), "{text:?}");
        }
        for text in ["tokio", "io tok", "tok io x", "tok", "", "   "] {
            assert!(!matches(text, &wanted), "{text:?}");
        }
        assert!(matches("   ", &key("")));
    }

    fn a_query_text() -> impl Strategy<Value = String> {
        prop_oneof![
            "[a-zA-Z0-9 \t\n\u{b}.-]{0,10}",
            "[a-cA-C \t\u{a0}\u{2003}éÉ\u{301}İΣß.]{0,10}",
        ]
    }

    proptest! {
        #[test]
        fn matches_agrees_with_comparing_keys(
            text in a_query_text(),
            query in a_query_text(),
            same in any::<bool>(),
        ) {
            let query = if same { text.to_uppercase() } else { query };
            let wanted = key(&query);
            prop_assert_eq!(matches(&text, &wanted), key(&text) == wanted);
        }

        #[test]
        fn without_a_remembered_directory_the_order_is_ranks(
            candidates in a_candidate_set(),
            query in proptest::sample::select(QUERIES),
            current in proptest::sample::select(CURRENT),
        ) {
            let expected = paths(&ranked(query, current, &candidates));
            let normal = ranked(query, current, &candidates);
            prop_assert!(!applies(&normal, None));
            let promoted = promote(normal, None);
            prop_assert_eq!(paths(&promoted), expected);
        }

        #[test]
        fn a_ranked_remembered_directory_comes_first_and_the_others_keep_their_order(
            candidates in a_candidate_set(),
            query in proptest::sample::select(QUERIES),
            current in proptest::sample::select(CURRENT),
            pick in any::<proptest::sample::Index>(),
            upper in any::<bool>(),
        ) {
            let normal = ranked(query, current, &candidates);
            prop_assume!(!normal.is_empty());
            let chosen = pick.index(normal.len());
            let remembered = cased(&normal[chosen].candidate.path, upper);
            let mut expected = paths(&normal);
            let head = expected.remove(chosen);
            expected.insert(0, head);
            prop_assert!(applies(&normal, Some(&remembered)));
            let promoted = promote(normal, Some(&remembered));
            prop_assert_eq!(paths(&promoted), expected);
        }

        #[test]
        fn an_unranked_remembered_directory_leaves_the_order_unchanged(
            candidates in a_candidate_set(),
            query in proptest::sample::select(QUERIES),
            current in proptest::sample::select(CURRENT),
            pick in any::<proptest::sample::Index>(),
        ) {
            let normal = ranked(query, current, &candidates);
            let ranked_paths: HashSet<String> = normal
                .iter()
                .map(|scored| scored.candidate.path.to_lowercase())
                .collect();
            let mut absent: Vec<String> = candidates
                .iter()
                .map(|candidate| candidate.path.clone())
                .filter(|path| !ranked_paths.contains(&path.to_lowercase()))
                .collect();
            absent.push("/nowhere/ombi".to_owned());
            let remembered = absent[pick.index(absent.len())].clone();
            let expected = paths(&normal);
            prop_assert!(!applies(&normal, Some(&remembered)));
            let promoted = promote(normal, Some(&remembered));
            prop_assert_eq!(paths(&promoted), expected);
        }

        #[test]
        fn promotion_is_deterministic_and_keeps_every_scored_exactly_once(
            (candidates, shuffled) in a_candidate_set()
                .prop_flat_map(|set| (Just(set.clone()), Just(set).prop_shuffle())),
            query in proptest::sample::select(QUERIES),
            current in proptest::sample::select(CURRENT),
            remembered in proptest::option::of(
                (proptest::sample::select(FOLDERS), proptest::sample::select(NAMES))
                    .prop_map(|(folder, name)| format!("{folder}/{name}")),
            ),
        ) {
            let normal = ranked(query, current, &candidates);
            let before: Vec<Scored> = normal.clone();
            let once = promote(normal.clone(), remembered.as_deref());
            let twice = promote(normal, remembered.as_deref());
            prop_assert_eq!(&once, &twice);
            let reordered = promote(ranked(query, current, &shuffled), remembered.as_deref());
            prop_assert_eq!(paths(&once), paths(&reordered));
            prop_assert_eq!(once.len(), before.len());
            for scored in &before {
                let copies = once.iter().filter(|other| *other == scored).count();
                prop_assert_eq!(copies, 1);
            }
        }

        #[test]
        fn d1_holds_among_every_directory_but_the_remembered_one(
            candidates in a_candidate_set(),
            query in proptest::sample::select(QUERIES),
            current in proptest::sample::select(CURRENT),
            pick in any::<proptest::sample::Index>(),
        ) {
            let normal = ranked(query, current, &candidates);
            prop_assume!(!normal.is_empty());
            let remembered = normal[pick.index(normal.len())].candidate.path.clone();
            let promoted = promote(normal, Some(&remembered));
            prop_assert_eq!(promoted[0].candidate.path.as_str(), remembered.as_str());
            for pair in promoted[1..].windows(2) {
                prop_assert!(pair[0].score >= pair[1].score);
                if pair[0].score == pair[1].score {
                    prop_assert!(pair[0].candidate.last_visit >= pair[1].candidate.last_visit);
                }
            }
        }
    }
}
