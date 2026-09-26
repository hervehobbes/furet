use std::cmp::Ordering;

use crate::clock::Timestamp;
use crate::stage1_nucleo::NucleoScorer;
use crate::{stage1, stage2};

/// Which stage of the engine produced a candidate's score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    One,
    Two,
}

/// The stage-1 scorer: the SPEC §7.2 reference engine, or nucleo (SPEC-v2 §20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Engine {
    #[default]
    Reference,
    Nucleo,
}

impl Engine {
    /// The name `config.toml`, `--engine`, and `--explain` use.
    pub fn name(self) -> &'static str {
        match self {
            Engine::Reference => "reference",
            Engine::Nucleo => "nucleo",
        }
    }
}

/// A directory the engine may rank: what to match on, when it was last
/// visited, and whether it is soft-deleted (SPEC §10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub path: String,
    pub name: String,
    pub folder: Option<String>,
    pub last_visit: Timestamp,
    pub missing: bool,
}

/// A candidate that matched the query, with its score and its stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scored<'a> {
    pub candidate: &'a Candidate,
    pub score: u32,
    pub stage: Stage,
}

enum Scorer {
    Reference,
    Nucleo(NucleoScorer),
}

impl Scorer {
    fn new(engine: Engine) -> Self {
        match engine {
            Engine::Reference => Scorer::Reference,
            Engine::Nucleo => Scorer::Nucleo(NucleoScorer::new()),
        }
    }

    fn dispatch(
        &mut self,
        query: &str,
        candidate: &Candidate,
        typo_min_length: usize,
    ) -> Option<(u32, Stage)> {
        let folder = candidate.folder.as_deref();
        let stage_one = match self {
            Scorer::Reference => stage1::score(query, &candidate.name, folder),
            Scorer::Nucleo(scorer) => scorer.score(query, &candidate.name, folder),
        };
        if let Some(found) = stage_one {
            return Some((found, Stage::One));
        }
        stage2::score(query, &candidate.name, typo_min_length).map(|found| (found, Stage::Two))
    }
}

/// Scores one candidate with `engine`'s stage 1, falling back to stage 2
/// only once stage 1 returned no match at all.
pub fn dispatch(
    query: &str,
    candidate: &Candidate,
    typo_min_length: usize,
    engine: Engine,
) -> Option<(u32, Stage)> {
    Scorer::new(engine).dispatch(query, candidate, typo_min_length)
}

/// Ranks the matching candidates in SPEC §8 order, after dropping the
/// missing ones and `current_dir` itself.
pub fn rank<'a>(
    query: &str,
    current_dir: &str,
    candidates: &'a [Candidate],
    typo_min_length: usize,
    engine: Engine,
) -> Vec<Scored<'a>> {
    let mut scorer = Scorer::new(engine);
    let mut ranked: Vec<Scored<'a>> = candidates
        .iter()
        .filter(|candidate| !candidate.missing && !same_path(&candidate.path, current_dir))
        .filter_map(|candidate| {
            scorer
                .dispatch(query, candidate, typo_min_length)
                .map(|(score, stage)| Scored {
                    candidate,
                    score,
                    stage,
                })
        })
        .collect();
    ranked.sort_by(|left, right| compare(&key(left), &key(right)));
    ranked
}

/// A SPEC section 8 sort criterion, in the order the comparator applies them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TieBreak {
    Score,
    Recency,
    NameLength,
    Path,
}

/// First criterion on which the winner and the runner-up of an already
/// ranked list differ; `None` when there is no runner-up.
pub fn deciding_criterion(ranked: &[Scored]) -> Option<TieBreak> {
    let winner = key(ranked.first()?);
    let runner_up = key(ranked.get(1)?);
    if winner.score != runner_up.score {
        return Some(TieBreak::Score);
    }
    if winner.last_visit != runner_up.last_visit {
        return Some(TieBreak::Recency);
    }
    if winner.name_len != runner_up.name_len {
        return Some(TieBreak::NameLength);
    }
    Some(TieBreak::Path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RankKey<'a> {
    score: u32,
    last_visit: Timestamp,
    name_len: usize,
    path: &'a str,
}

fn key<'a>(scored: &Scored<'a>) -> RankKey<'a> {
    RankKey {
        score: scored.score,
        last_visit: scored.candidate.last_visit,
        name_len: scored.candidate.name.chars().count(),
        path: &scored.candidate.path,
    }
}

fn compare(left: &RankKey, right: &RankKey) -> Ordering {
    right
        .score
        .cmp(&left.score)
        .then_with(|| right.last_visit.cmp(&left.last_visit))
        .then_with(|| left.name_len.cmp(&right.name_len))
        .then_with(|| left.path.cmp(right.path))
}

pub(crate) fn same_path(left: &str, right: &str) -> bool {
    left.to_lowercase() == right.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{
        Candidate, Engine, RankKey, Scored, Stage, TieBreak, compare, deciding_criterion, dispatch,
        key, rank, same_path,
    };
    use crate::clock::Timestamp;
    use crate::{stage1, stage2};
    use proptest::prelude::*;
    use std::cmp::Ordering;
    use std::collections::HashSet;

    fn at(hours: i64) -> Timestamp {
        Timestamp::from_unix_seconds(1_700_000_000 - hours * 3_600)
    }

    fn dir(path: &str, name: &str, folder: &str, hours: i64) -> Candidate {
        Candidate {
            path: path.to_owned(),
            name: name.to_owned(),
            folder: Some(folder.to_owned()),
            last_visit: at(hours),
            missing: false,
        }
    }

    fn paths(query: &str, current_dir: &str, candidates: &[Candidate]) -> Vec<String> {
        rank(
            query,
            current_dir,
            candidates,
            stage2::TYPO_MIN_QUERY_LEN,
            Engine::Reference,
        )
        .iter()
        .map(|scored| scored.candidate.path.clone())
        .collect()
    }

    #[test]
    fn dispatch_prefers_stage_one_when_it_matches() {
        let candidate = dir("/dev/tokio", "tokio", "/dev", 1);
        let found = dispatch(
            "tokio",
            &candidate,
            stage2::TYPO_MIN_QUERY_LEN,
            Engine::Reference,
        );
        assert_eq!(found, Some((90, Stage::One)));
        assert!(found.map(|(score, _)| score) > Some(stage1::SCORE_FLOOR));
    }

    #[test]
    fn dispatch_falls_back_to_stage_two_when_stage_one_fails() {
        let candidate = dir("/dev/tokio", "tokio", "/dev", 1);
        assert_eq!(stage1::score("tokoi", "tokio", Some("/dev")), None);
        assert_eq!(
            dispatch(
                "tokoi",
                &candidate,
                stage2::TYPO_MIN_QUERY_LEN,
                Engine::Reference
            ),
            Some((2, Stage::Two))
        );
    }

    #[test]
    fn dispatch_returns_nothing_when_both_stages_fail() {
        let candidate = dir("/dev/tokio", "tokio", "/dev", 1);
        assert_eq!(
            dispatch(
                "zigzag",
                &candidate,
                stage2::TYPO_MIN_QUERY_LEN,
                Engine::Reference
            ),
            None
        );
        assert_eq!(
            dispatch(
                "",
                &candidate,
                stage2::TYPO_MIN_QUERY_LEN,
                Engine::Reference
            ),
            None
        );
    }

    #[test]
    fn a_higher_score_ranks_first_however_old_it_is() {
        let candidates = [
            dir("/dev/tokyo", "tokyo", "/dev", 1),
            dir("/dev/tokio", "tokio", "/dev", 240),
        ];
        assert_eq!(
            paths("tokio", "", &candidates),
            ["/dev/tokio", "/dev/tokyo"]
        );
    }

    #[test]
    fn recency_breaks_a_score_tie() {
        let candidates = [
            dir("/archive/helix", "helix", "/archive", 120),
            dir("/dev/helix", "helix", "/dev", 1),
        ];
        assert_eq!(
            dispatch(
                "helix",
                &candidates[0],
                stage2::TYPO_MIN_QUERY_LEN,
                Engine::Reference
            ),
            dispatch(
                "helix",
                &candidates[1],
                stage2::TYPO_MIN_QUERY_LEN,
                Engine::Reference
            )
        );
        assert_eq!(
            paths("helix", "", &candidates),
            ["/dev/helix", "/archive/helix"]
        );
    }

    #[test]
    fn name_length_breaks_a_tie_once_recency_is_equal() {
        let candidates = [
            dir("/dev/rip-grep", "rip-grep", "/dev", 2),
            dir("/dev/ripgrep", "ripgrep", "/dev", 2),
        ];
        assert_eq!(
            dispatch(
                "ri",
                &candidates[0],
                stage2::TYPO_MIN_QUERY_LEN,
                Engine::Reference
            ),
            dispatch(
                "ri",
                &candidates[1],
                stage2::TYPO_MIN_QUERY_LEN,
                Engine::Reference
            )
        );
        assert_eq!(
            paths("ri", "", &candidates),
            ["/dev/ripgrep", "/dev/rip-grep"]
        );
    }

    #[test]
    fn the_path_breaks_a_tie_once_the_name_length_is_equal() {
        let candidates = [
            dir("/dev/helix", "helix", "/dev", 2),
            dir("/archive/helix", "helix", "/archive", 2),
        ];
        assert_eq!(
            dispatch(
                "helix",
                &candidates[0],
                stage2::TYPO_MIN_QUERY_LEN,
                Engine::Reference
            ),
            dispatch(
                "helix",
                &candidates[1],
                stage2::TYPO_MIN_QUERY_LEN,
                Engine::Reference
            )
        );
        assert_eq!(
            paths("helix", "", &candidates),
            ["/archive/helix", "/dev/helix"]
        );
    }

    #[test]
    fn a_missing_candidate_never_reaches_the_ranking() {
        let mut candidates = [
            dir("/dev/tokio", "tokio", "/dev", 1),
            dir("/dev/tokyo", "tokyo", "/dev", 120),
        ];
        candidates[0].missing = true;
        assert_eq!(paths("tokio", "", &candidates), ["/dev/tokyo"]);
    }

    #[test]
    fn the_current_directory_is_excluded_whatever_its_case() {
        let candidates = [
            dir("/dev/helix", "helix", "/dev", 1),
            dir("/archive/helix", "helix", "/archive", 120),
        ];
        assert_eq!(
            paths("helix", "/DEV/Helix", &candidates),
            ["/archive/helix"]
        );
        assert!(same_path("/DEV/Helix", "/dev/helix"));
        assert!(!same_path("/dev/helix", "/dev/helixx"));
    }

    #[test]
    fn a_query_matching_nothing_ranks_nothing() {
        let candidates = [
            dir("/dev/tokio", "tokio", "/dev", 1),
            dir("/dev/helix", "helix", "/dev", 2),
        ];
        assert!(paths("zigzag", "", &candidates).is_empty());
    }

    #[test]
    fn every_stage_one_match_ranks_before_every_stage_two_match() {
        let candidates = [
            dir("/dev/tokyo", "tokyo", "/dev", 1),
            dir("/dev/tokio", "tokio", "/dev", 240),
        ];
        let ranked = rank(
            "tokio",
            "",
            &candidates,
            stage2::TYPO_MIN_QUERY_LEN,
            Engine::Reference,
        );
        assert_eq!(ranked.len(), 2);
        assert_eq!(
            ranked.iter().map(|scored| scored.stage).collect::<Vec<_>>(),
            [Stage::One, Stage::Two]
        );
    }

    fn ranked<'a>(candidates: &'a [Candidate], scores: &[u32]) -> Vec<Scored<'a>> {
        candidates
            .iter()
            .zip(scores)
            .map(|(candidate, score)| Scored {
                candidate,
                score: *score,
                stage: Stage::One,
            })
            .collect()
    }

    #[test]
    fn deciding_criterion_needs_a_runner_up() {
        let candidates = [dir("/dev/tokio", "tokio", "/dev", 1)];
        assert_eq!(deciding_criterion(&[]), None);
        assert_eq!(deciding_criterion(&ranked(&candidates, &[90])), None);
    }

    #[test]
    fn a_score_difference_decides_before_anything_else() {
        let candidates = [
            dir("/dev/tokio", "tokio", "/dev", 240),
            dir("/dev/tokei", "tokei", "/dev", 1),
        ];
        assert_eq!(
            deciding_criterion(&ranked(&candidates, &[90, 38])),
            Some(TieBreak::Score)
        );
    }

    #[test]
    fn recency_decides_a_genuine_score_tie() {
        let candidates = [
            dir("/dev/helix", "helix", "/dev", 1),
            dir("/archive/helix", "helix", "/archive", 120),
        ];
        assert_eq!(
            deciding_criterion(&ranked(&candidates, &[90, 90])),
            Some(TieBreak::Recency)
        );
    }

    #[test]
    fn name_length_decides_once_the_score_and_the_recency_tie() {
        let candidates = [
            dir("/dev/ripgrep", "ripgrep", "/dev", 2),
            dir("/dev/rip-grep", "rip-grep", "/dev", 2),
        ];
        assert_eq!(
            deciding_criterion(&ranked(&candidates, &[40, 40])),
            Some(TieBreak::NameLength)
        );
    }

    #[test]
    fn the_path_decides_once_every_earlier_criterion_ties() {
        let candidates = [
            dir("/archive/helix", "helix", "/archive", 2),
            dir("/dev/helix", "helix", "/dev", 2),
        ];
        assert_eq!(
            deciding_criterion(&ranked(&candidates, &[90, 90])),
            Some(TieBreak::Path)
        );
    }

    #[test]
    fn deciding_criterion_reads_the_real_ranking_the_same_way() {
        let candidates = [
            dir("/dev/tokyo", "tokyo", "/dev", 1),
            dir("/dev/tokio", "tokio", "/dev", 240),
        ];
        assert_eq!(
            deciding_criterion(&rank(
                "tokio",
                "",
                &candidates,
                stage2::TYPO_MIN_QUERY_LEN,
                Engine::Reference
            )),
            Some(TieBreak::Score)
        );
        let tied = [
            dir("/archive/helix", "helix", "/archive", 120),
            dir("/dev/helix", "helix", "/dev", 1),
        ];
        assert_eq!(
            deciding_criterion(&rank(
                "helix",
                "",
                &tied,
                stage2::TYPO_MIN_QUERY_LEN,
                Engine::Reference
            )),
            Some(TieBreak::Recency)
        );
    }

    static NAMES: &[&str] = &["tokio", "tokei", "helix", "neovim", "ripgrep", "Réunions"];
    static FOLDERS: &[&str] = &["/dev", "/DEV", "/archive"];
    static QUERIES: &[&str] = &["ri", "tok", "tokio", "tokoi", "helix", "reunions", "zigzag"];
    static CURRENT: &[&str] = &["", "/dev/tokio", "/DEV/TOKIO", "/archive/helix"];

    fn a_candidate() -> impl Strategy<Value = Candidate> {
        (
            proptest::sample::select(FOLDERS),
            proptest::sample::select(NAMES),
            0i64..96,
            any::<bool>(),
        )
            .prop_map(|(folder, name, hours, missing)| Candidate {
                path: format!("{folder}/{name}"),
                name: name.to_owned(),
                folder: Some(folder.to_owned()),
                last_visit: at(hours),
                missing,
            })
    }

    fn a_candidate_set() -> impl Strategy<Value = Vec<Candidate>> {
        proptest::collection::vec(a_candidate(), 0..8).prop_map(|candidates| {
            let mut seen: HashSet<String> = HashSet::new();
            candidates
                .into_iter()
                .filter(|candidate| seen.insert(candidate.path.clone()))
                .collect()
        })
    }

    fn a_candidate_set_and_a_permutation() -> impl Strategy<Value = (Vec<Candidate>, Vec<Candidate>)>
    {
        a_candidate_set()
            .prop_flat_map(|candidates| (Just(candidates.clone()), Just(candidates).prop_shuffle()))
    }

    fn a_rank_key() -> impl Strategy<Value = (u32, i64, usize, String)> {
        (0u32..200, 0i64..1_000, 1usize..24, "[a-z/-]{1,10}")
    }

    fn rank_key(parts: &(u32, i64, usize, String)) -> RankKey<'_> {
        RankKey {
            score: parts.0,
            last_visit: Timestamp::from_unix_seconds(parts.1),
            name_len: parts.2,
            path: &parts.3,
        }
    }

    proptest! {
        #[test]
        fn d1_no_recency_difference_ever_inverts_a_strict_score_difference(
            first in a_rank_key(),
            second in a_rank_key(),
        ) {
            prop_assume!(first.0 != second.0);
            let (high, low) = if first.0 > second.0 {
                (rank_key(&first), rank_key(&second))
            } else {
                (rank_key(&second), rank_key(&first))
            };
            prop_assert_eq!(compare(&high, &low), Ordering::Less);
            prop_assert_eq!(compare(&low, &high), Ordering::Greater);
        }

        #[test]
        fn d1_ranked_scores_never_increase(
            candidates in a_candidate_set(),
            query in proptest::sample::select(QUERIES),
            current in proptest::sample::select(CURRENT),
        ) {
            let ranked = rank(query, current, &candidates, stage2::TYPO_MIN_QUERY_LEN, Engine::Reference);
            for pair in ranked.windows(2) {
                prop_assert!(pair[0].score >= pair[1].score);
            }
        }

        #[test]
        fn a_stage_one_match_always_outscores_a_stage_two_match(
            candidates in a_candidate_set(),
            query in proptest::sample::select(QUERIES),
            current in proptest::sample::select(CURRENT),
        ) {
            let ranked = rank(query, current, &candidates, stage2::TYPO_MIN_QUERY_LEN, Engine::Reference);
            for scored in &ranked {
                match scored.stage {
                    Stage::One => prop_assert!(scored.score >= stage1::SCORE_FLOOR),
                    Stage::Two => prop_assert!((1..=stage2::SCORE_CAP).contains(&scored.score)),
                }
            }
            let last_stage_one = ranked.iter().rposition(|s| s.stage == Stage::One);
            let first_stage_two = ranked.iter().position(|s| s.stage == Stage::Two);
            if let (Some(one), Some(two)) = (last_stage_one, first_stage_two) {
                prop_assert!(one < two);
            }
        }

        #[test]
        fn the_deciding_criterion_names_a_criterion_that_really_differs(
            candidates in a_candidate_set(),
            query in proptest::sample::select(QUERIES),
            current in proptest::sample::select(CURRENT),
        ) {
            let ranked = rank(query, current, &candidates, stage2::TYPO_MIN_QUERY_LEN, Engine::Reference);
            let (winner, runner_up) = match deciding_criterion(&ranked) {
                None => {
                    prop_assert!(ranked.len() < 2);
                    return Ok(());
                }
                Some(criterion) => {
                    prop_assert!(ranked.len() >= 2);
                    let (winner, runner_up) = (key(&ranked[0]), key(&ranked[1]));
                    match criterion {
                        TieBreak::Score => prop_assert!(winner.score > runner_up.score),
                        TieBreak::Recency => {
                            prop_assert_eq!(winner.score, runner_up.score);
                            prop_assert!(winner.last_visit > runner_up.last_visit);
                        }
                        TieBreak::NameLength => {
                            prop_assert_eq!(winner.score, runner_up.score);
                            prop_assert_eq!(winner.last_visit, runner_up.last_visit);
                            prop_assert!(winner.name_len < runner_up.name_len);
                        }
                        TieBreak::Path => {
                            prop_assert_eq!(winner.score, runner_up.score);
                            prop_assert_eq!(winner.last_visit, runner_up.last_visit);
                            prop_assert_eq!(winner.name_len, runner_up.name_len);
                            prop_assert!(winner.path < runner_up.path);
                        }
                    }
                    (winner, runner_up)
                }
            };
            prop_assert_eq!(compare(&winner, &runner_up), Ordering::Less);
        }

        #[test]
        fn the_comparator_is_a_strict_total_order(
            first in a_rank_key(),
            second in a_rank_key(),
            third in a_rank_key(),
        ) {
            let (left, middle, right) = (rank_key(&first), rank_key(&second), rank_key(&third));
            prop_assert_eq!(compare(&left, &left), Ordering::Equal);
            prop_assert_eq!(compare(&left, &middle), compare(&middle, &left).reverse());
            if left.path != middle.path {
                prop_assert_ne!(compare(&left, &middle), Ordering::Equal);
            }
            if compare(&left, &middle) == Ordering::Less
                && compare(&middle, &right) == Ordering::Less
            {
                prop_assert_eq!(compare(&left, &right), Ordering::Less);
            }
        }

        #[test]
        fn the_ranked_order_does_not_depend_on_the_input_order(
            (candidates, shuffled) in a_candidate_set_and_a_permutation(),
            query in proptest::sample::select(QUERIES),
            current in proptest::sample::select(CURRENT),
        ) {
            let expected: Vec<&str> = rank(query, current, &candidates, stage2::TYPO_MIN_QUERY_LEN, Engine::Reference)
                .iter()
                .map(|scored| scored.candidate.path.as_str())
                .collect();
            let found: Vec<&str> = rank(query, current, &shuffled, stage2::TYPO_MIN_QUERY_LEN, Engine::Reference)
                .iter()
                .map(|scored| scored.candidate.path.as_str())
                .collect();
            prop_assert_eq!(expected, found);
        }

        #[test]
        fn a_ranked_candidate_is_never_missing_nor_the_current_directory(
            candidates in a_candidate_set(),
            query in proptest::sample::select(QUERIES),
            current in proptest::sample::select(CURRENT),
        ) {
            for scored in rank(query, current, &candidates, stage2::TYPO_MIN_QUERY_LEN, Engine::Reference) {
                prop_assert!(!scored.candidate.missing);
                prop_assert!(!same_path(&scored.candidate.path, current));
                prop_assert_eq!(
                    dispatch(query, scored.candidate, stage2::TYPO_MIN_QUERY_LEN, Engine::Reference).map(|(score, _)| score),
                    Some(scored.score)
                );
            }
        }

        #[test]
        fn the_ranking_keeps_every_matching_candidate_exactly_once(
            candidates in a_candidate_set(),
            query in proptest::sample::select(QUERIES),
            current in proptest::sample::select(CURRENT),
        ) {
            let kept = candidates
                .iter()
                .filter(|candidate| !candidate.missing && !same_path(&candidate.path, current))
                .filter(|candidate| dispatch(query, candidate, stage2::TYPO_MIN_QUERY_LEN, Engine::Reference).is_some())
                .count();
            let ranked = rank(query, current, &candidates, stage2::TYPO_MIN_QUERY_LEN, Engine::Reference);
            prop_assert_eq!(ranked.len(), kept);
            let distinct: HashSet<&str> = ranked
                .iter()
                .map(|scored| scored.candidate.path.as_str())
                .collect();
            prop_assert_eq!(distinct.len(), kept);
            for scored in &ranked {
                prop_assert_eq!(key(scored).path, scored.candidate.path.as_str());
            }
        }
    }

    static NUCLEO_NAMES: &[&str] = &[
        "tokio",
        "tokei",
        "helix",
        "neovim",
        "ripgrep",
        "Réunions",
        "Reunions",
        "my-dev",
        "d-e-v",
        "ToolWindows",
    ];
    static NUCLEO_QUERIES: &[&str] = &[
        "ri",
        "tok",
        "tokio",
        "tokoi",
        "helix",
        "réunions",
        "dev",
        "tw",
        "neo vim",
        "zigzag",
    ];

    fn a_nucleo_candidate_set() -> impl Strategy<Value = Vec<Candidate>> {
        proptest::collection::vec(
            (
                proptest::sample::select(FOLDERS),
                proptest::sample::select(NUCLEO_NAMES),
                0i64..96,
                any::<bool>(),
            ),
            0..8,
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
                .filter(|candidate| seen.insert(candidate.path.clone()))
                .collect()
        })
    }

    fn nucleo_paths<'a>(ranked: &[Scored<'a>]) -> Vec<&'a str> {
        ranked
            .iter()
            .map(|scored| scored.candidate.path.as_str())
            .collect()
    }

    proptest! {
        #[test]
        fn nucleo_every_stage_one_match_ranks_before_every_stage_two_match(
            candidates in a_nucleo_candidate_set(),
            query in proptest::sample::select(NUCLEO_QUERIES),
            current in proptest::sample::select(CURRENT),
        ) {
            let ranked = rank(query, current, &candidates, stage2::TYPO_MIN_QUERY_LEN, Engine::Nucleo);
            for scored in &ranked {
                match scored.stage {
                    Stage::One => {
                        prop_assert!(scored.score >= stage1::SCORE_FLOOR);
                        prop_assert!(scored.score > stage2::SCORE_CAP);
                    }
                    Stage::Two => prop_assert!((1..=stage2::SCORE_CAP).contains(&scored.score)),
                }
            }
            let last_stage_one = ranked.iter().rposition(|s| s.stage == Stage::One);
            let first_stage_two = ranked.iter().position(|s| s.stage == Stage::Two);
            if let (Some(one), Some(two)) = (last_stage_one, first_stage_two) {
                prop_assert!(one < two);
            }
        }

        #[test]
        fn nucleo_ranked_keys_are_strictly_ordered_whatever_the_input_order(
            (candidates, shuffled) in a_nucleo_candidate_set().prop_flat_map(|candidates| {
                (Just(candidates.clone()), Just(candidates).prop_shuffle())
            }),
            query in proptest::sample::select(NUCLEO_QUERIES),
            current in proptest::sample::select(CURRENT),
        ) {
            let ranked = rank(query, current, &candidates, stage2::TYPO_MIN_QUERY_LEN, Engine::Nucleo);
            for (index, earlier) in ranked.iter().enumerate() {
                for later in &ranked[index + 1..] {
                    prop_assert_eq!(compare(&key(earlier), &key(later)), Ordering::Less);
                    prop_assert_eq!(compare(&key(later), &key(earlier)), Ordering::Greater);
                }
            }
            let reordered = rank(query, current, &shuffled, stage2::TYPO_MIN_QUERY_LEN, Engine::Nucleo);
            prop_assert_eq!(nucleo_paths(&ranked), nucleo_paths(&reordered));
        }

        #[test]
        fn nucleo_d1_making_every_weaker_match_more_recent_never_lifts_it(
            candidates in a_nucleo_candidate_set(),
            query in proptest::sample::select(NUCLEO_QUERIES),
        ) {
            let ranked = rank(query, "", &candidates, stage2::TYPO_MIN_QUERY_LEN, Engine::Nucleo);
            for pair in ranked.windows(2) {
                prop_assert!(pair[0].score >= pair[1].score);
            }
            let inverted: Vec<Candidate> = candidates
                .iter()
                .map(|candidate| {
                    let score = dispatch(query, candidate, stage2::TYPO_MIN_QUERY_LEN, Engine::Nucleo)
                        .map_or(0, |(score, _)| i64::from(score));
                    Candidate {
                        last_visit: at(score - 1_000),
                        ..candidate.clone()
                    }
                })
                .collect();
            let reranked = rank(query, "", &inverted, stage2::TYPO_MIN_QUERY_LEN, Engine::Nucleo);
            let score_of = |path: &str| {
                ranked
                    .iter()
                    .find(|scored| scored.candidate.path == path)
                    .map(|scored| scored.score)
            };
            for (index, earlier) in reranked.iter().enumerate() {
                for later in &reranked[index + 1..] {
                    prop_assert!(earlier.score >= later.score);
                    prop_assert_eq!(score_of(&earlier.candidate.path), Some(earlier.score));
                    if earlier.score > later.score {
                        prop_assert!(earlier.candidate.last_visit < later.candidate.last_visit);
                    }
                }
            }
        }

        #[test]
        fn nucleo_rank_scores_every_candidate_like_a_standalone_dispatch(
            candidates in a_nucleo_candidate_set(),
            query in proptest::sample::select(NUCLEO_QUERIES),
            current in proptest::sample::select(CURRENT),
        ) {
            let ranked = rank(query, current, &candidates, stage2::TYPO_MIN_QUERY_LEN, Engine::Nucleo);
            let kept = candidates
                .iter()
                .filter(|candidate| !candidate.missing && !same_path(&candidate.path, current))
                .filter(|candidate| {
                    dispatch(query, candidate, stage2::TYPO_MIN_QUERY_LEN, Engine::Nucleo).is_some()
                })
                .count();
            prop_assert_eq!(ranked.len(), kept);
            for scored in &ranked {
                prop_assert_eq!(
                    dispatch(query, scored.candidate, stage2::TYPO_MIN_QUERY_LEN, Engine::Nucleo),
                    Some((scored.score, scored.stage))
                );
            }
        }
    }
}
