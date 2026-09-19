use crate::normalize::Normalized;

const TOKEN_BASE: i64 = 10;
const CONSECUTIVE_BONUS: i64 = 8;
const WORD_START_BONUS: i64 = 10;
const NAME_START_BONUS: i64 = 8;
const PREFIX_BONUS: i64 = 10;
const DENSITY_FACTOR: i64 = 10;
const GAP_PENALTY: i64 = 1;
const ORDER_BONUS: i64 = 5;
const FOLDER_BONUS: u32 = 2;

/// Lowest score stage 1 can return; it sits above the 1..3 range of stage 2.
pub const SCORE_FLOOR: u32 = 4;

/// Stage-1 score of `query` against a candidate `name` and its joined parent
/// segments. `None` when a query token is not a subsequence of the name.
pub fn score(query: &str, name: &str, folder: Option<&str>) -> Option<u32> {
    let tokens: Vec<Normalized> = query.split_whitespace().map(Normalized::new).collect();
    if tokens.is_empty() {
        return None;
    }
    let candidate = Normalized::new(name);
    let mut total: i64 = 0;
    let mut starts: Vec<usize> = Vec::with_capacity(tokens.len());
    for token in &tokens {
        let matched = match_token(token, &candidate)?;
        total += matched.total();
        starts.push(matched.start);
    }
    if starts.windows(2).all(|pair| pair[0] < pair[1]) {
        total += ORDER_BONUS;
    }
    let floored = u32::try_from(total.max(i64::from(SCORE_FLOOR))).unwrap_or(u32::MAX);
    if folder.is_some_and(|part| matches_folder(&tokens, part)) {
        return Some(floored.saturating_add(FOLDER_BONUS));
    }
    Some(floored)
}

/// One query token's stage-1 contribution, split into base and bonuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenBreakdown {
    pub token: String,
    pub base: i64,
    pub length: i64,
    pub placement: i64,
    pub prefix: i64,
    pub density: i64,
}

/// Every component behind a stage-1 score, plus the score itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakdown {
    pub tokens: Vec<TokenBreakdown>,
    pub order_bonus: i64,
    pub folder_bonus: i64,
    pub total: u32,
}

/// Same match as `score`, reporting each token's bonuses; `Some` exactly
/// when `score` is, with `total` equal to what `score` returned.
pub fn explain(query: &str, name: &str, folder: Option<&str>) -> Option<Breakdown> {
    let tokens: Vec<Normalized> = query.split_whitespace().map(Normalized::new).collect();
    let candidate = Normalized::new(name);
    let mut reported: Vec<TokenBreakdown> = Vec::with_capacity(tokens.len());
    let mut starts: Vec<usize> = Vec::with_capacity(tokens.len());
    for token in &tokens {
        let matched = match_token(token, &candidate)?;
        starts.push(matched.start);
        reported.push(TokenBreakdown {
            token: token.text(),
            base: TOKEN_BASE,
            length: matched.length,
            placement: matched.placement,
            prefix: matched.prefix,
            density: matched.density,
        });
    }
    let total = score(query, name, folder)?;
    let order_bonus = if starts.windows(2).all(|pair| pair[0] < pair[1]) {
        ORDER_BONUS
    } else {
        0
    };
    let folder_bonus = if folder.is_some_and(|part| matches_folder(&tokens, part)) {
        i64::from(FOLDER_BONUS)
    } else {
        0
    };
    Some(Breakdown {
        tokens: reported,
        order_bonus,
        folder_bonus,
        total,
    })
}

struct TokenMatch {
    length: i64,
    placement: i64,
    prefix: i64,
    density: i64,
    start: usize,
}

impl TokenMatch {
    fn total(&self) -> i64 {
        TOKEN_BASE + self.length + self.placement + self.prefix + self.density
    }
}

#[derive(Clone, Copy)]
struct Placement {
    score: i64,
    start: usize,
}

fn match_token(token: &Normalized, candidate: &Normalized) -> Option<TokenMatch> {
    if !is_subsequence(token.chars(), candidate.chars()) || token.is_empty() {
        return None;
    }
    let placement = best_placement(token, candidate)?;
    let length = token.len() as i64;
    let prefix = if candidate.chars().starts_with(token.chars()) {
        PREFIX_BONUS
    } else {
        0
    };
    let density = DENSITY_FACTOR * length / candidate.len() as i64;
    Some(TokenMatch {
        length,
        placement: placement.score,
        prefix,
        density,
        start: placement.start,
    })
}

fn is_subsequence(token: &[char], candidate: &[char]) -> bool {
    let mut remaining = candidate.iter();
    token
        .iter()
        .all(|wanted| remaining.any(|available| available == wanted))
}

fn best_placement(token: &Normalized, candidate: &Normalized) -> Option<Placement> {
    let characters = candidate.chars();
    let (first, rest) = token.chars().split_first()?;
    if characters.is_empty() {
        return None;
    }
    let mut previous: Vec<Option<Placement>> = vec![None; characters.len()];
    let mut current: Vec<Option<Placement>> = vec![None; characters.len()];
    for (index, available) in characters.iter().enumerate() {
        if available == first {
            let mut score = 0;
            if candidate.is_word_start(index) {
                score += WORD_START_BONUS;
            }
            if index == 0 {
                score += NAME_START_BONUS;
            }
            previous[index] = Some(Placement {
                score,
                start: index,
            });
        }
    }
    for wanted in rest {
        let mut running: Option<Placement> = None;
        for (index, available) in characters.iter().enumerate() {
            let earlier = index.checked_sub(1).and_then(|before| previous[before]);
            if let Some(earlier) = earlier {
                let lifted = earlier.score + GAP_PENALTY * (index as i64 - 1);
                if running.is_none_or(|best| lifted > best.score) {
                    running = Some(Placement {
                        score: lifted,
                        start: earlier.start,
                    });
                }
            }
            let mut placed = None;
            if index > 0 && available == wanted {
                let mut best = running.map(|carried| Placement {
                    score: carried.score - GAP_PENALTY * (index as i64 - 1),
                    start: carried.start,
                });
                if let Some(earlier) = earlier {
                    let consecutive = earlier.score + CONSECUTIVE_BONUS;
                    if best.is_none_or(|found| consecutive > found.score) {
                        best = Some(Placement {
                            score: consecutive,
                            start: earlier.start,
                        });
                    }
                }
                if let Some(mut found) = best {
                    if candidate.is_word_start(index) {
                        found.score += WORD_START_BONUS;
                    }
                    placed = Some(found);
                }
            }
            current[index] = placed;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous
        .iter()
        .flatten()
        .fold(None, |best: Option<Placement>, found| match best {
            Some(kept) if kept.score >= found.score => Some(kept),
            _ => Some(*found),
        })
}

fn matches_folder(tokens: &[Normalized], folder: &str) -> bool {
    let folder = Normalized::new(folder);
    tokens
        .iter()
        .all(|token| !token.is_empty() && is_subsequence(token.chars(), folder.chars()))
}

#[cfg(test)]
mod tests {
    use super::{
        Normalized, SCORE_FLOOR, TokenMatch, best_placement, explain, is_subsequence, match_token,
        score,
    };
    use proptest::prelude::*;

    fn matched(query: &str, name: &str) -> TokenMatch {
        match_token(&Normalized::new(query), &Normalized::new(name))
            .expect("the fixture query is a subsequence of the fixture name")
    }

    fn placement(query: &str, name: &str) -> (i64, usize) {
        let found = best_placement(&Normalized::new(query), &Normalized::new(name))
            .expect("the fixture query is a subsequence of the fixture name");
        (found.score, found.start)
    }

    #[test]
    fn an_exact_name_match_collects_every_bonus() {
        assert_eq!(score("tokio", "tokio", None), Some(90));
    }

    #[test]
    fn rejects_a_token_that_is_not_a_subsequence() {
        assert_eq!(score("zig", "tokio", None), None);
        assert_eq!(score("oot", "tokio", None), None);
        assert_eq!(score("helix", "", None), None);
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        assert_eq!(score("", "tokio", None), None);
        assert_eq!(score("   ", "tokio", None), None);
    }

    #[test]
    fn a_consecutive_run_outscores_a_gapped_placement() {
        assert_eq!(score("to", "tokio", None), Some(57));
        assert_eq!(score("tk", "tokio", None), Some(38));
    }

    #[test]
    fn the_gap_penalty_removes_one_point_per_skipped_character() {
        assert_eq!(placement("tz", "txz").0, 17);
        assert_eq!(placement("tz", "txxz").0, 16);
        assert_eq!(placement("tz", "txxxz").0, 15);
    }

    #[test]
    fn the_consecutive_bonus_adds_eight_per_adjacent_character() {
        assert_eq!(placement("t", "tokio").0, 18);
        assert_eq!(placement("to", "tokio").0, 26);
        assert_eq!(placement("tok", "tokio").0, 34);
    }

    #[test]
    fn a_word_start_after_a_separator_adds_ten() {
        assert_eq!(placement("rg", "rip-grep").0, 25);
        assert_eq!(placement("rg", "ripxgrep").0, 15);
    }

    #[test]
    fn a_camel_hump_read_on_the_original_string_adds_ten() {
        assert_eq!(placement("rg", "ripGrep").0, 26);
        assert_eq!(placement("rg", "ripgrep").0, 16);
    }

    #[test]
    fn the_name_start_bonus_needs_the_first_character_at_index_zero() {
        assert_eq!(placement("ab", "ab-").0, 26);
        assert_eq!(placement("ab", "-ab").0, 18);
    }

    #[test]
    fn the_prefix_bonus_needs_a_whole_string_prefix() {
        assert_eq!(matched("to", "tokio").prefix, 10);
        assert_eq!(matched("tokio", "tokio").prefix, 10);
        assert_eq!(matched("tk", "tokio").prefix, 0);
        assert_eq!(matched("ok", "tokio").prefix, 0);
    }

    #[test]
    fn the_density_bonus_uses_truncating_division() {
        assert_eq!(matched("tokio", "tokio").density, 10);
        assert_eq!(matched("to", "tokio").density, 4);
        assert_eq!(matched("neo", "neovim").density, 5);
        assert_eq!(matched("t", "tokei").density, 2);
    }

    #[test]
    fn the_optimal_placement_beats_the_first_greedy_one() {
        assert_eq!(placement("ab", "a-ab"), (18, 2));
    }

    #[test]
    fn every_token_must_match_the_name() {
        assert!(score("neo vim", "neovim", None).is_some());
        assert_eq!(score("neo zig", "neovim", None), None);
    }

    #[test]
    fn the_order_bonus_needs_strictly_increasing_token_starts() {
        assert_eq!(score("neo vim", "neovim", None), Some(101));
        assert_eq!(score("vim neo", "neovim", None), Some(96));
    }

    #[test]
    fn the_score_is_floored_at_four() {
        let name = format!("a{}z", "b".repeat(50));
        assert_eq!(score("az", &name, None), Some(SCORE_FLOOR));
    }

    #[test]
    fn the_folder_bonus_is_added_on_top_of_the_floor() {
        let name = format!("a{}z", "b".repeat(50));
        assert_eq!(score("az", &name, Some("azure")), Some(SCORE_FLOOR + 2));
    }

    #[test]
    fn the_folder_bonus_adds_two_when_the_query_also_matches_the_folder() {
        assert_eq!(score("tk", "tokei", None), Some(38));
        assert_eq!(score("tk", "tokei", Some("dev\\toolkit")), Some(40));
        assert_eq!(score("tk", "tokei", Some("dev\\helix")), Some(38));
    }

    #[test]
    fn an_accented_name_matches_its_unaccented_query() {
        assert_eq!(score("reunions", "Réunions", None), Some(117));
        assert_eq!(
            score("reunions", "Réunions", None),
            score("reunions", "reunions", None)
        );
        assert_eq!(score("réunions", "Reunions", None), Some(117));
    }

    #[test]
    fn explain_reports_the_same_total_as_score_on_an_exact_match() {
        let breakdown = explain("tokio", "tokio", None).expect("the exact match is explained");
        assert_eq!(breakdown.total, 90);
        assert_eq!(Some(breakdown.total), score("tokio", "tokio", None));
        assert_eq!(breakdown.tokens.len(), 1);
        let token = &breakdown.tokens[0];
        assert_eq!(token.token, "tokio");
        assert_eq!(
            (
                token.base,
                token.length,
                token.placement,
                token.prefix,
                token.density
            ),
            (10, 5, 50, 10, 10)
        );
        assert_eq!(breakdown.order_bonus, 5);
        assert_eq!(breakdown.folder_bonus, 0);
    }

    #[test]
    fn explain_reports_the_order_and_folder_bonuses_separately() {
        let ordered = explain("neo vim", "neovim", None).expect("both tokens match");
        assert_eq!(ordered.total, 101);
        assert_eq!(ordered.order_bonus, 5);
        let reversed = explain("vim neo", "neovim", None).expect("both tokens still match");
        assert_eq!(reversed.total, 96);
        assert_eq!(reversed.order_bonus, 0);
        let foldered = explain("tk", "tokei", Some("dev\\toolkit")).expect("the token matches");
        assert_eq!(foldered.total, 40);
        assert_eq!(foldered.folder_bonus, 2);
        let plain = explain("tk", "tokei", Some("dev\\helix")).expect("the token matches");
        assert_eq!(plain.total, 38);
        assert_eq!(plain.folder_bonus, 0);
    }

    #[test]
    fn explain_rejects_exactly_what_score_rejects() {
        assert!(explain("zig", "tokio", None).is_none());
        assert!(explain("", "tokio", None).is_none());
        assert!(explain("   ", "tokio", None).is_none());
        assert!(explain("neo zig", "neovim", None).is_none());
    }

    static ALPHABET: &[char] = &[
        'a', 'b', 'c', 'k', 'o', 't', 'i', 'R', 'é', '-', '_', '.', ' ',
    ];

    fn a_candidate_and_one_of_its_subsequences() -> impl Strategy<Value = (String, String)> {
        proptest::collection::vec(proptest::sample::select(ALPHABET), 1..16)
            .prop_flat_map(|characters| {
                let length = characters.len();
                (
                    Just(characters),
                    proptest::collection::vec(any::<bool>(), length),
                )
            })
            .prop_map(|(characters, mask)| {
                let name: String = characters.iter().collect();
                let query: String = characters
                    .iter()
                    .zip(mask)
                    .filter(|(_, keep)| *keep)
                    .map(|(character, _)| *character)
                    .collect();
                (name, query)
            })
    }

    proptest! {
        #[test]
        fn a_stage_one_score_never_falls_below_the_floor(
            query in "[a-zRé._ -]{0,12}",
            name in "[a-zRé._ -]{0,16}",
            folder in "[a-zRé._\\\\ -]{0,16}",
        ) {
            if let Some(found) = score(&query, &name, Some(&folder)) {
                prop_assert!(found >= SCORE_FLOOR);
            }
            if let Some(found) = score(&query, &name, None) {
                prop_assert!(found >= SCORE_FLOOR);
            }
        }

        #[test]
        fn a_subsequence_query_always_matches_and_respects_the_floor(
            (name, query) in a_candidate_and_one_of_its_subsequences(),
        ) {
            prop_assume!(!query.trim().is_empty());
            let found = score(&query, &name, None);
            prop_assert!(found.is_some());
            prop_assert!(found.unwrap_or(0) >= SCORE_FLOOR);
        }

        #[test]
        fn the_same_pair_always_produces_the_same_score(
            query in "[a-zRé._ -]{0,12}",
            name in "[a-zRé._ -]{0,16}",
            folder in "[a-zRé._\\\\ -]{0,16}",
        ) {
            let first = score(&query, &name, Some(&folder));
            let second = score(&query, &name, Some(&folder));
            prop_assert_eq!(first, second);
            prop_assert_eq!(score(&query, &name, None), score(&query, &name, None));
        }

        #[test]
        fn explain_always_agrees_with_score(
            query in "[a-zRé._ -]{0,12}",
            name in "[a-zRé._ -]{0,16}",
            folder in "[a-zRé._\\\\ -]{0,16}",
        ) {
            prop_assert_eq!(
                explain(&query, &name, Some(&folder)).map(|found| found.total),
                score(&query, &name, Some(&folder))
            );
            prop_assert_eq!(
                explain(&query, &name, None).map(|found| found.total),
                score(&query, &name, None)
            );
        }

        #[test]
        fn explain_agrees_with_score_on_every_subsequence_pair(
            (name, query) in a_candidate_and_one_of_its_subsequences(),
        ) {
            let breakdown = explain(&query, &name, None);
            prop_assert_eq!(
                breakdown.as_ref().map(|found| found.total),
                score(&query, &name, None)
            );
            if let Some(found) = breakdown {
                prop_assert_eq!(found.tokens.len(), query.split_whitespace().count());
            }
        }

        #[test]
        fn greedy_rejection_agrees_with_the_dynamic_program(
            query in "[a-zRé._ -]{1,10}",
            name in "[a-zRé._ -]{1,14}",
        ) {
            let token = Normalized::new(&query);
            let candidate = Normalized::new(&name);
            prop_assume!(!token.is_empty() && !candidate.is_empty());
            prop_assert_eq!(
                is_subsequence(token.chars(), candidate.chars()),
                best_placement(&token, &candidate).is_some()
            );
        }
    }
}
