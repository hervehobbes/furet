use crate::rank::{Candidate, Scored, Stage};

/// Most entries SPEC section 9's menu ever shows.
pub const MENU_MAX_ENTRIES: usize = 9;

/// What SPEC section 9 concludes from an already-ranked candidate list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision<'a> {
    Jump(&'a Candidate),
    Menu(Vec<&'a Candidate>),
    None,
}

/// Applies SPEC section 9 to `rank`'s output: a stage-1 best always jumps, a
/// stage-2 best jumps when alone at its distance and opens a menu otherwise.
pub fn decide<'a>(ranked: &[Scored<'a>]) -> Decision<'a> {
    let Some(top) = ranked.first() else {
        return Decision::None;
    };
    if top.stage == Stage::One {
        return Decision::Jump(top.candidate);
    }
    let tied: Vec<&'a Candidate> = ranked
        .iter()
        .take_while(|scored| scored.score == top.score)
        .take(MENU_MAX_ENTRIES)
        .map(|scored| scored.candidate)
        .collect();
    if tied.len() < 2 {
        return Decision::Jump(top.candidate);
    }
    Decision::Menu(tied)
}

/// Renders SPEC section 9's console menu; the caller writes it to stderr,
/// never to stdout.
pub fn render_menu(candidates: &[&Candidate]) -> String {
    let mut rendered = String::from("Choose a directory:\n");
    for (index, candidate) in candidates.iter().enumerate() {
        rendered.push_str(&format!("  {}) {}\n", index + 1, candidate.path));
    }
    rendered.push_str("Enter to confirm, Esc to cancel\n");
    rendered
}

/// Reads a menu answer: a 1-based index within `1..=count`, or `None` for
/// every other input, which cancels.
pub fn selection(input: &str, count: usize) -> Option<usize> {
    let index: usize = input.trim().parse().ok()?;
    (1..=count).contains(&index).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::{Decision, MENU_MAX_ENTRIES, decide, render_menu, selection};
    use crate::clock::Timestamp;
    use crate::rank::{Candidate, Engine, Scored, Stage, rank};
    use crate::stage2::TYPO_MIN_QUERY_LEN;
    use crate::{stage1, stage2};
    use proptest::prelude::*;
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

    fn scored<'a>(candidates: &'a [Candidate], scores: &[u32]) -> Vec<Scored<'a>> {
        candidates
            .iter()
            .zip(scores)
            .map(|(candidate, score)| Scored {
                candidate,
                score: *score,
                stage: stage_of(*score),
            })
            .collect()
    }

    fn stage_of(score: u32) -> Stage {
        if score >= stage1::SCORE_FLOOR {
            Stage::One
        } else {
            Stage::Two
        }
    }

    fn numbered(count: usize) -> Vec<Candidate> {
        (0..count)
            .map(|index| {
                let name = format!("d{index}");
                dir(&format!("/dev/{name}"), &name, "/dev", index as i64)
            })
            .collect()
    }

    fn chosen_paths<'a>(decision: &'a Decision<'a>) -> Vec<&'a str> {
        match decision {
            Decision::Menu(candidates) => candidates
                .iter()
                .map(|candidate| candidate.path.as_str())
                .collect(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn an_empty_ranking_decides_nothing() {
        assert_eq!(decide(&[]), Decision::None);
    }

    #[test]
    fn a_stage_one_best_jumps_even_when_another_stage_one_ties_it() {
        let candidates = numbered(3);
        let ranked = scored(&candidates, &[90, 90, 2]);
        assert_eq!(decide(&ranked), Decision::Jump(&candidates[0]));
        let alone = scored(&candidates, &[90, 12, 2]);
        assert_eq!(decide(&alone), Decision::Jump(&candidates[0]));
    }

    #[test]
    fn a_stage_one_best_jumps_even_when_nine_stage_one_candidates_tie_it() {
        let candidates = numbered(9);
        let ranked = scored(&candidates, &[stage1::SCORE_FLOOR; 9]);
        assert_eq!(decide(&ranked), Decision::Jump(&candidates[0]));
    }

    #[test]
    fn a_lone_stage_two_best_distance_jumps() {
        let candidates = numbered(3);
        let ranked = scored(&candidates, &[3, 2, 1]);
        assert_eq!(decide(&ranked), Decision::Jump(&candidates[0]));
    }

    #[test]
    fn tied_stage_two_candidates_open_a_menu_of_exactly_those_candidates() {
        let candidates = numbered(4);
        let ranked = scored(&candidates, &[2, 2, 1, 1]);
        let decision = decide(&ranked);
        assert_eq!(
            decision,
            Decision::Menu(vec![&candidates[0], &candidates[1]])
        );
        assert_eq!(chosen_paths(&decision), ["/dev/d0", "/dev/d1"]);
    }

    #[test]
    fn a_menu_keeps_at_most_nine_tied_candidates_in_rank_order() {
        let candidates = numbered(12);
        let ranked = scored(&candidates, &[stage2::SCORE_CAP; 12]);
        let decision = decide(&ranked);
        assert_eq!(chosen_paths(&decision).len(), MENU_MAX_ENTRIES);
        assert_eq!(
            chosen_paths(&decision),
            [
                "/dev/d0", "/dev/d1", "/dev/d2", "/dev/d3", "/dev/d4", "/dev/d5", "/dev/d6",
                "/dev/d7", "/dev/d8"
            ]
        );
    }

    #[test]
    fn a_stage_two_menu_holds_only_the_leading_run_of_equal_scores() {
        let candidates = numbered(5);
        let ranked = scored(&candidates, &[3, 3, 3, 2, 1]);
        assert_eq!(
            chosen_paths(&decide(&ranked)),
            ["/dev/d0", "/dev/d1", "/dev/d2"]
        );
    }

    #[test]
    fn render_menu_prints_one_numbered_line_per_candidate() {
        let candidates = [
            dir("c:\\dev\\tokio", "tokio", "c:\\dev", 1),
            dir("c:\\dev\\tokei", "tokei", "c:\\dev", 2),
        ];
        let shown: Vec<&Candidate> = candidates.iter().collect();
        assert_eq!(
            render_menu(&shown[..1]),
            "Choose a directory:\n  1) c:\\dev\\tokio\nEnter to confirm, Esc to cancel\n"
        );
        assert_eq!(
            render_menu(&shown),
            "Choose a directory:\n  1) c:\\dev\\tokio\n  2) c:\\dev\\tokei\nEnter to confirm, Esc to cancel\n"
        );
    }

    #[test]
    fn render_menu_numbers_nine_entries_from_one_to_nine() {
        let candidates = numbered(MENU_MAX_ENTRIES);
        let shown: Vec<&Candidate> = candidates.iter().collect();
        let expected = "Choose a directory:\n\
             \x20 1) /dev/d0\n\
             \x20 2) /dev/d1\n\
             \x20 3) /dev/d2\n\
             \x20 4) /dev/d3\n\
             \x20 5) /dev/d4\n\
             \x20 6) /dev/d5\n\
             \x20 7) /dev/d6\n\
             \x20 8) /dev/d7\n\
             \x20 9) /dev/d8\n\
             Enter to confirm, Esc to cancel\n";
        assert_eq!(render_menu(&shown), expected);
    }

    #[test]
    fn a_digit_in_range_selects_and_anything_else_cancels() {
        assert_eq!(selection("1", 2), Some(1));
        assert_eq!(selection(" 2\r\n", 2), Some(2));
        assert_eq!(selection("3", 2), None);
        assert_eq!(selection("0", 2), None);
        assert_eq!(selection("-1", 2), None);
        assert_eq!(selection("nope", 2), None);
        assert_eq!(selection("", 2), None);
        assert_eq!(selection("\n", 2), None);
        assert_eq!(selection("1 2", 2), None);
    }

    static NAMES: &[&str] = &["tokio", "tokei", "helix", "neovim"];
    static FOLDERS: &[&str] = &["/dev", "/archive", "/opt"];
    static QUERIES: &[&str] = &["tok", "tokoi", "helxi", "neovm", "zigzag"];

    fn a_candidate() -> impl Strategy<Value = Candidate> {
        (
            proptest::sample::select(FOLDERS),
            proptest::sample::select(NAMES),
            0i64..96,
        )
            .prop_map(|(folder, name, hours)| dir(&format!("{folder}/{name}"), name, folder, hours))
    }

    fn a_candidate_set() -> impl Strategy<Value = Vec<Candidate>> {
        proptest::collection::vec(a_candidate(), 0..10).prop_map(|candidates| {
            let mut seen: HashSet<String> = HashSet::new();
            candidates
                .into_iter()
                .filter(|candidate| seen.insert(candidate.path.clone()))
                .collect()
        })
    }

    fn a_score_ladder() -> impl Strategy<Value = Vec<u32>> {
        prop_oneof![
            proptest::collection::vec(1u32..6, 0..14),
            proptest::collection::vec(1u32..3, 9..14),
        ]
        .prop_map(|mut scores| {
            scores.sort_unstable_by(|left, right| right.cmp(left));
            scores
        })
    }

    proptest! {
        #[test]
        fn a_menu_always_holds_two_to_nine_equally_scored_stage_two_candidates(
            scores in a_score_ladder(),
        ) {
            let candidates = numbered(scores.len());
            let ranked = scored(&candidates, &scores);
            if let Decision::Menu(shown) = decide(&ranked) {
                prop_assert!((2..=MENU_MAX_ENTRIES).contains(&shown.len()));
                for candidate in &shown {
                    let entry = ranked
                        .iter()
                        .find(|scored| std::ptr::eq(scored.candidate, *candidate));
                    prop_assert!(entry.is_some());
                    if let Some(entry) = entry {
                        prop_assert_eq!(entry.stage, Stage::Two);
                        prop_assert!(entry.score <= stage2::SCORE_CAP);
                        prop_assert!(entry.score < stage1::SCORE_FLOOR);
                        prop_assert_eq!(entry.score, ranked[0].score);
                    }
                }
            }
        }

        #[test]
        fn a_stage_one_best_never_opens_a_menu(scores in a_score_ladder()) {
            prop_assume!(scores.first().is_some_and(|best| *best >= stage1::SCORE_FLOOR));
            let candidates = numbered(scores.len());
            let ranked = scored(&candidates, &scores);
            prop_assert_eq!(decide(&ranked), Decision::Jump(&candidates[0]));
        }

        #[test]
        fn the_same_ranking_always_decides_the_same_way(scores in a_score_ladder()) {
            let candidates = numbered(scores.len());
            let ranked = scored(&candidates, &scores);
            prop_assert_eq!(decide(&ranked), decide(&ranked));
        }

        #[test]
        fn a_decision_over_a_real_ranking_keeps_the_stage_and_order_of_rank(
            candidates in a_candidate_set(),
            query in proptest::sample::select(QUERIES),
        ) {
            let ranked = rank(query, "", &candidates, TYPO_MIN_QUERY_LEN, Engine::Reference);
            let decision = decide(&ranked);
            prop_assert_eq!(&decision, &decide(&ranked));
            match &decision {
                Decision::None => prop_assert!(ranked.is_empty()),
                Decision::Jump(candidate) => {
                    prop_assert_eq!(&candidate.path, &ranked[0].candidate.path);
                }
                Decision::Menu(shown) => {
                    prop_assert!((2..=MENU_MAX_ENTRIES).contains(&shown.len()));
                    prop_assert_eq!(ranked[0].stage, Stage::Two);
                    for (entry, candidate) in ranked.iter().zip(shown) {
                        prop_assert_eq!(entry.stage, Stage::Two);
                        prop_assert!(entry.score < stage1::SCORE_FLOOR);
                        prop_assert_eq!(entry.score, ranked[0].score);
                        prop_assert_eq!(&entry.candidate.path, &candidate.path);
                    }
                }
            }
        }
    }
}
