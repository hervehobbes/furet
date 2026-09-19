use crate::normalize::Normalized;

const WINDOW_SLACK: usize = 2;
const MAX_DISTANCE: usize = 2;

/// Shortest normalized mono-token query stage 2 accepts; the default behind
/// the future `typo_min_length` override.
pub const TYPO_MIN_QUERY_LEN: usize = 4;

/// Highest score stage 2 can return; it sits below `stage1::SCORE_FLOOR`.
pub const SCORE_CAP: u32 = 3;

/// Stage-2 score in `1..=3`, a fallback the caller reaches only once
/// `stage1::score` returned `None`; optimal string alignment distance.
pub fn score(query: &str, name: &str) -> Option<u32> {
    let mut tokens = query.split_whitespace();
    let token = Normalized::new(tokens.next()?);
    if tokens.next().is_some() || token.len() < TYPO_MIN_QUERY_LEN {
        return None;
    }
    let candidate = Normalized::new(name);
    let distance = best_window_distance(token.chars(), candidate.chars());
    if distance > MAX_DISTANCE {
        return None;
    }
    Some(SCORE_CAP - distance as u32)
}

fn best_window_distance(query: &[char], candidate: &[char]) -> usize {
    let shortest = query
        .len()
        .saturating_sub(WINDOW_SLACK)
        .min(candidate.len());
    let longest = (query.len() + WINDOW_SLACK).min(candidate.len());
    let mut best = query.len().max(candidate.len());
    for length in shortest..=longest {
        for start in 0..=(candidate.len() - length) {
            let window = &candidate[start..start + length];
            best = best.min(optimal_string_alignment(query, window));
        }
    }
    best
}

fn optimal_string_alignment(left: &[char], right: &[char]) -> usize {
    if left.is_empty() {
        return right.len();
    }
    if right.is_empty() {
        return left.len();
    }
    let width = right.len() + 1;
    let mut two_back: Vec<usize> = vec![0; width];
    let mut previous: Vec<usize> = (0..width).collect();
    let mut current: Vec<usize> = vec![0; width];
    for row in 1..=left.len() {
        current[0] = row;
        for column in 1..width {
            let cost = usize::from(left[row - 1] != right[column - 1]);
            let mut value = (current[column - 1] + 1)
                .min(previous[column] + 1)
                .min(previous[column - 1] + cost);
            if row > 1
                && column > 1
                && left[row - 1] == right[column - 2]
                && left[row - 2] == right[column - 1]
            {
                value = value.min(two_back[column - 2] + cost);
            }
            current[column] = value;
        }
        std::mem::swap(&mut two_back, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_DISTANCE, SCORE_CAP, TYPO_MIN_QUERY_LEN, best_window_distance,
        optimal_string_alignment, score,
    };
    use crate::normalize::Normalized;
    use crate::stage1;
    use proptest::prelude::*;

    fn distance(left: &str, right: &str) -> usize {
        optimal_string_alignment(
            Normalized::new(left).chars(),
            Normalized::new(right).chars(),
        )
    }

    fn window_distance(query: &str, name: &str) -> usize {
        best_window_distance(
            Normalized::new(query).chars(),
            Normalized::new(name).chars(),
        )
    }

    #[test]
    fn a_query_shorter_than_the_threshold_never_matches() {
        assert_eq!(TYPO_MIN_QUERY_LEN, 4);
        assert_eq!(distance("zig", "zib"), 1);
        assert_eq!(score("zig", "zib"), None);
        assert_eq!(score("hlix", "helix"), Some(2));
        assert_eq!(score("hli", "helix"), None);
    }

    #[test]
    fn a_multi_token_query_never_matches() {
        assert_eq!(score("tokoi", "tokio"), Some(2));
        assert_eq!(score("tokoi tokoi", "tokio"), None);
        assert_eq!(score("tokio tokio", "tokio"), None);
    }

    #[test]
    fn an_empty_query_or_an_empty_name_never_matches() {
        assert_eq!(score("", "tokio"), None);
        assert_eq!(score("   ", "tokio"), None);
        assert_eq!(score("tokio", ""), None);
    }

    #[test]
    fn a_one_edit_typo_scores_two() {
        assert_eq!(window_distance("tokoi", "tokio"), 1);
        assert_eq!(score("tokoi", "tokio"), Some(2));
        assert_eq!(score("helyx", "helix"), Some(2));
    }

    #[test]
    fn an_adjacent_transposition_counts_as_a_single_edit() {
        assert_eq!(distance("ripgrpe", "ripgrep"), 1);
        assert_eq!(score("ripgrpe", "ripgrep"), Some(2));
        assert_eq!(distance("zellij", "zellij"), 0);
    }

    #[test]
    fn the_alignment_variant_never_edits_the_same_span_twice() {
        assert_eq!(distance("ca", "abc"), 3);
        assert_eq!(distance("ab", "ba"), 1);
    }

    #[test]
    fn a_distance_of_two_scores_one_and_a_distance_of_three_scores_nothing() {
        assert_eq!(window_distance("tokio", "tokei"), MAX_DISTANCE);
        assert_eq!(score("tokio", "tokei"), Some(1));
        assert_eq!(window_distance("zellij", "zelda"), 3);
        assert_eq!(score("zellij", "zelda"), None);
    }

    #[test]
    fn a_sliding_window_matches_a_fragment_of_a_longer_name() {
        assert_eq!(window_distance("ripgrep", "rip-grep-all"), 1);
        assert_eq!(score("ripgrep", "rip-grep-all"), Some(2));
        assert_eq!(score("ripgrep", "ripgrep-all-the-things"), Some(SCORE_CAP));
    }

    #[test]
    fn an_accented_name_is_normalized_before_the_distance() {
        assert_eq!(score("reunoins", "Réunions"), Some(2));
        assert_eq!(score("réunoins", "Reunions"), Some(2));
        assert_eq!(score("reunoins", "Réunions"), score("reunoins", "reunions"));
    }

    static ALPHABET: &[char] = &['a', 'b', 'c', 'k', 'o', 't', 'i', 'R', 'é', '-', '_', '.'];

    fn a_name_and_a_query_within_two_deletions() -> impl Strategy<Value = (String, String)> {
        proptest::collection::vec(proptest::sample::select(ALPHABET), 6..14)
            .prop_flat_map(|characters| {
                let length = characters.len();
                (
                    Just(characters),
                    proptest::collection::vec(0..length, 0..=MAX_DISTANCE),
                )
            })
            .prop_map(|(characters, dropped)| {
                let name: String = characters.iter().collect();
                let query: String = characters
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !dropped.contains(index))
                    .map(|(_, character)| *character)
                    .collect();
                (name, query)
            })
    }

    proptest! {
        #[test]
        fn a_stage_one_score_always_beats_a_stage_two_score(
            query in "[a-zRé._ -]{0,12}",
            name in "[a-zRé._ -]{0,16}",
        ) {
            if let (Some(first), Some(second)) =
                (stage1::score(&query, &name, None), score(&query, &name))
            {
                prop_assert!(first > second);
            }
        }

        #[test]
        fn a_near_miss_scoring_in_both_stages_still_ranks_stage_one_first(
            (name, query) in a_name_and_a_query_within_two_deletions(),
        ) {
            let first = stage1::score(&query, &name, None);
            let second = score(&query, &name);
            prop_assert!(first.is_some());
            prop_assert!(second.is_some());
            prop_assert!(first.unwrap_or(0) >= stage1::SCORE_FLOOR);
            prop_assert!(first.unwrap_or(0) > second.unwrap_or(u32::MAX));
        }

        #[test]
        fn a_stage_two_score_always_lies_between_one_and_the_cap(
            query in "[a-zRé._ -]{0,12}",
            name in "[a-zRé._ -]{0,16}",
        ) {
            if let Some(found) = score(&query, &name) {
                prop_assert!((1..=SCORE_CAP).contains(&found));
                prop_assert!(found < stage1::SCORE_FLOOR);
            }
        }

        #[test]
        fn the_same_pair_always_produces_the_same_stage_two_score(
            query in "[a-zRé._ -]{0,12}",
            name in "[a-zRé._ -]{0,16}",
        ) {
            prop_assert_eq!(score(&query, &name), score(&query, &name));
        }
    }
}
