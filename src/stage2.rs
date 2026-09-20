use crate::normalize::Normalized;

const WINDOW_SLACK: usize = 2;

/// Largest optimal string alignment distance stage 2 still accepts, for a
/// query of at least `LONG_QUERY_MIN_LEN` normalized characters.
pub const MAX_DISTANCE: usize = 2;

/// Largest distance stage 2 accepts below `LONG_QUERY_MIN_LEN`, where two
/// edits would rewrite half the query.
pub const SHORT_QUERY_MAX_DISTANCE: usize = 1;

/// Query length, in normalized characters, from which `MAX_DISTANCE` applies.
pub const LONG_QUERY_MIN_LEN: usize = 6;

/// Shortest normalized mono-token query stage 2 accepts; the default behind
/// the `typo_min_length` config override (SPEC section 16).
pub const TYPO_MIN_QUERY_LEN: usize = 4;

/// Highest score stage 2 can return; it sits below `stage1::SCORE_FLOOR`.
pub const SCORE_CAP: u32 = 3;

/// Stage-2 score in `1..=3`, a fallback the caller reaches only once
/// `stage1::score` returned `None`; optimal string alignment distance.
pub fn score(query: &str, name: &str, min_length: usize) -> Option<u32> {
    let token = eligible_token(query, min_length)?;
    let candidate = Normalized::new(name);
    let distance = best_window_distance(token.chars(), candidate.chars());
    if distance > max_distance(token.len()) {
        return None;
    }
    Some(SCORE_CAP - distance as u32)
}

/// Raw best-window distance whenever the query is eligible for stage 2 at
/// all, including the distances `score` rejects as too far.
pub fn explain(query: &str, name: &str, min_length: usize) -> Option<usize> {
    let token = eligible_token(query, min_length)?;
    let candidate = Normalized::new(name);
    Some(best_window_distance(token.chars(), candidate.chars()))
}

/// Largest distance `score` accepts for a query of `query_len` normalized
/// characters: 1 below `LONG_QUERY_MIN_LEN`, `MAX_DISTANCE` from there on.
pub fn max_distance(query_len: usize) -> usize {
    if query_len < LONG_QUERY_MIN_LEN {
        SHORT_QUERY_MAX_DISTANCE
    } else {
        MAX_DISTANCE
    }
}

/// The distance ceiling `score` applies to `query`, measured on its first
/// normalized token; for reports, it ignores `min_length` eligibility.
pub fn query_max_distance(query: &str) -> usize {
    let token = query.split_whitespace().next().unwrap_or_default();
    max_distance(Normalized::new(token).len())
}

fn eligible_token(query: &str, min_length: usize) -> Option<Normalized> {
    let mut tokens = query.split_whitespace();
    let token = Normalized::new(tokens.next()?);
    if tokens.next().is_some() || token.len() < min_length {
        return None;
    }
    Some(token)
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
        LONG_QUERY_MIN_LEN, MAX_DISTANCE, SCORE_CAP, SHORT_QUERY_MAX_DISTANCE, TYPO_MIN_QUERY_LEN,
        best_window_distance, explain, max_distance, optimal_string_alignment, query_max_distance,
        score,
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
        assert_eq!(score("zig", "zib", TYPO_MIN_QUERY_LEN), None);
        assert_eq!(score("hlix", "helix", TYPO_MIN_QUERY_LEN), Some(2));
        assert_eq!(score("hli", "helix", TYPO_MIN_QUERY_LEN), None);
    }

    #[test]
    fn a_multi_token_query_never_matches() {
        assert_eq!(score("tokoi", "tokio", TYPO_MIN_QUERY_LEN), Some(2));
        assert_eq!(score("tokoi tokoi", "tokio", TYPO_MIN_QUERY_LEN), None);
        assert_eq!(score("tokio tokio", "tokio", TYPO_MIN_QUERY_LEN), None);
    }

    #[test]
    fn an_empty_query_or_an_empty_name_never_matches() {
        assert_eq!(score("", "tokio", TYPO_MIN_QUERY_LEN), None);
        assert_eq!(score("   ", "tokio", TYPO_MIN_QUERY_LEN), None);
        assert_eq!(score("tokio", "", TYPO_MIN_QUERY_LEN), None);
    }

    #[test]
    fn a_one_edit_typo_scores_two() {
        assert_eq!(window_distance("tokoi", "tokio"), 1);
        assert_eq!(score("tokoi", "tokio", TYPO_MIN_QUERY_LEN), Some(2));
        assert_eq!(score("helyx", "helix", TYPO_MIN_QUERY_LEN), Some(2));
    }

    #[test]
    fn an_adjacent_transposition_counts_as_a_single_edit() {
        assert_eq!(distance("ripgrpe", "ripgrep"), 1);
        assert_eq!(score("ripgrpe", "ripgrep", TYPO_MIN_QUERY_LEN), Some(2));
        assert_eq!(distance("zellij", "zellij"), 0);
    }

    #[test]
    fn the_alignment_variant_never_edits_the_same_span_twice() {
        assert_eq!(distance("ca", "abc"), 3);
        assert_eq!(distance("ab", "ba"), 1);
    }

    #[test]
    fn a_distance_of_two_scores_one_and_a_distance_of_three_scores_nothing() {
        assert_eq!(window_distance("meovin", "neovim"), MAX_DISTANCE);
        assert_eq!(score("meovin", "neovim", TYPO_MIN_QUERY_LEN), Some(1));
        assert_eq!(window_distance("zellij", "zelda"), 3);
        assert_eq!(score("zellij", "zelda", TYPO_MIN_QUERY_LEN), None);
    }

    #[test]
    fn the_maximum_distance_depends_on_the_query_length() {
        assert_eq!(LONG_QUERY_MIN_LEN, 6);
        assert_eq!(max_distance(4), SHORT_QUERY_MAX_DISTANCE);
        assert_eq!(max_distance(5), SHORT_QUERY_MAX_DISTANCE);
        assert_eq!(max_distance(6), MAX_DISTANCE);
        assert_eq!(max_distance(10), MAX_DISTANCE);
    }

    #[test]
    fn five_characters_refuse_the_distance_six_characters_accept() {
        assert_eq!(window_distance("tokio", "tokei"), 2);
        assert_eq!(score("tokio", "tokei", TYPO_MIN_QUERY_LEN), None);
        assert_eq!(explain("tokio", "tokei", TYPO_MIN_QUERY_LEN), Some(2));
        assert_eq!(query_max_distance("tokio"), SHORT_QUERY_MAX_DISTANCE);
        assert_eq!(query_max_distance("neovim"), MAX_DISTANCE);
    }

    #[test]
    fn a_short_query_still_accepts_a_single_edit() {
        assert_eq!(score("tokoi", "tokio", TYPO_MIN_QUERY_LEN), Some(2));
        assert_eq!(score("hlix", "helix", TYPO_MIN_QUERY_LEN), Some(2));
        assert_eq!(score("obmi", "ombi", TYPO_MIN_QUERY_LEN), Some(2));
    }

    #[test]
    fn a_four_character_query_refuses_every_two_edit_name() {
        for name in ["outils", "prompts", "toolwindows", "temporaire"] {
            assert_eq!(explain("ombi", name, TYPO_MIN_QUERY_LEN), Some(2), "{name}");
            assert_eq!(score("ombi", name, TYPO_MIN_QUERY_LEN), None, "{name}");
        }
    }

    #[test]
    fn an_accented_query_is_measured_after_normalization() {
        let five = "re\u{301}uni";
        let six = "re\u{301}unio";
        assert_eq!(five.chars().count(), 6);
        assert_eq!(Normalized::new(five).len(), 5);
        assert_eq!(query_max_distance(five), SHORT_QUERY_MAX_DISTANCE);
        assert_eq!(Normalized::new(six).len(), 6);
        assert_eq!(query_max_distance(six), MAX_DISTANCE);
        assert_eq!(explain(five, "rexnu", TYPO_MIN_QUERY_LEN), Some(2));
        assert_eq!(score(five, "rexnu", TYPO_MIN_QUERY_LEN), None);
    }

    #[test]
    fn a_sliding_window_matches_a_fragment_of_a_longer_name() {
        assert_eq!(window_distance("ripgrep", "rip-grep-all"), 1);
        assert_eq!(
            score("ripgrep", "rip-grep-all", TYPO_MIN_QUERY_LEN),
            Some(2)
        );
        assert_eq!(
            score("ripgrep", "ripgrep-all-the-things", TYPO_MIN_QUERY_LEN),
            Some(SCORE_CAP)
        );
    }

    #[test]
    fn an_accented_name_is_normalized_before_the_distance() {
        assert_eq!(score("reunoins", "Réunions", TYPO_MIN_QUERY_LEN), Some(2));
        assert_eq!(score("réunoins", "Reunions", TYPO_MIN_QUERY_LEN), Some(2));
        assert_eq!(
            score("reunoins", "Réunions", TYPO_MIN_QUERY_LEN),
            score("reunoins", "reunions", TYPO_MIN_QUERY_LEN)
        );
    }

    #[test]
    fn explain_reports_the_distance_behind_every_accepted_score() {
        assert_eq!(explain("tokoi", "tokio", TYPO_MIN_QUERY_LEN), Some(1));
        assert_eq!(
            explain("tokio", "tokei", TYPO_MIN_QUERY_LEN),
            Some(MAX_DISTANCE)
        );
        assert_eq!(
            explain("ripgrep", "ripgrep-all-the-things", TYPO_MIN_QUERY_LEN),
            Some(0)
        );
        assert_eq!(explain("hlix", "helix", TYPO_MIN_QUERY_LEN), Some(1));
    }

    #[test]
    fn explain_still_reports_a_distance_score_rejects_as_too_far() {
        assert_eq!(score("zellij", "zelda", TYPO_MIN_QUERY_LEN), None);
        assert_eq!(explain("zellij", "zelda", TYPO_MIN_QUERY_LEN), Some(3));
        assert_eq!(score("tokio", "", TYPO_MIN_QUERY_LEN), None);
        assert_eq!(explain("tokio", "", TYPO_MIN_QUERY_LEN), Some(5));
    }

    #[test]
    fn explain_reports_nothing_when_the_query_is_not_eligible_at_all() {
        assert_eq!(explain("zig", "zib", TYPO_MIN_QUERY_LEN), None);
        assert_eq!(explain("hli", "helix", TYPO_MIN_QUERY_LEN), None);
        assert_eq!(explain("tokoi tokoi", "tokio", TYPO_MIN_QUERY_LEN), None);
        assert_eq!(explain("", "tokio", TYPO_MIN_QUERY_LEN), None);
        assert_eq!(explain("   ", "tokio", TYPO_MIN_QUERY_LEN), None);
    }

    static ALPHABET: &[char] = &['a', 'b', 'c', 'k', 'o', 't', 'i', 'R', 'é', '-', '_', '.'];

    fn a_name_and_a_query_within_two_deletions() -> impl Strategy<Value = (String, String)> {
        proptest::collection::vec(
            proptest::sample::select(ALPHABET),
            LONG_QUERY_MIN_LEN + MAX_DISTANCE..14,
        )
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
                (stage1::score(&query, &name, None), score(&query, &name, TYPO_MIN_QUERY_LEN))
            {
                prop_assert!(first > second);
            }
        }

        #[test]
        fn a_near_miss_scoring_in_both_stages_still_ranks_stage_one_first(
            (name, query) in a_name_and_a_query_within_two_deletions(),
        ) {
            let first = stage1::score(&query, &name, None);
            let second = score(&query, &name, TYPO_MIN_QUERY_LEN);
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
            if let Some(found) = score(&query, &name, TYPO_MIN_QUERY_LEN) {
                prop_assert!((1..=SCORE_CAP).contains(&found));
                prop_assert!(found < stage1::SCORE_FLOOR);
            }
        }

        #[test]
        fn explain_reports_the_distance_every_accepted_score_encodes(
            query in "[a-zRé._ -]{0,12}",
            name in "[a-zRé._ -]{0,16}",
        ) {
            if let Some(found) = score(&query, &name, TYPO_MIN_QUERY_LEN) {
                let distance = usize::try_from(SCORE_CAP - found).unwrap_or(usize::MAX);
                prop_assert_eq!(explain(&query, &name, TYPO_MIN_QUERY_LEN), Some(distance));
                prop_assert!(distance <= query_max_distance(&query));
            }
        }

        #[test]
        fn a_query_shorter_than_six_characters_never_accepts_two_edits(
            query in "[a-zRé._-]{0,5}",
            name in "[a-zRé._ -]{0,16}",
        ) {
            if score(&query, &name, TYPO_MIN_QUERY_LEN).is_some() {
                prop_assert!(Normalized::new(&query).len() < LONG_QUERY_MIN_LEN);
                let distance = explain(&query, &name, TYPO_MIN_QUERY_LEN);
                prop_assert!(
                    matches!(distance, Some(found) if found <= SHORT_QUERY_MAX_DISTANCE),
                    "{distance:?}"
                );
            }
        }

        #[test]
        fn explain_and_score_agree_on_eligibility_and_on_the_threshold(
            query in "[a-zRé._ -]{0,12}",
            name in "[a-zRé._ -]{0,16}",
        ) {
            let ceiling = query_max_distance(&query);
            match explain(&query, &name, TYPO_MIN_QUERY_LEN) {
                None => prop_assert_eq!(score(&query, &name, TYPO_MIN_QUERY_LEN), None),
                Some(distance) if distance > ceiling => {
                    prop_assert_eq!(score(&query, &name, TYPO_MIN_QUERY_LEN), None);
                }
                Some(distance) => {
                    let expected = SCORE_CAP - u32::try_from(distance).unwrap_or(u32::MAX);
                    prop_assert_eq!(score(&query, &name, TYPO_MIN_QUERY_LEN), Some(expected));
                }
            }
        }

        #[test]
        fn the_same_pair_always_produces_the_same_stage_two_score(
            query in "[a-zRé._ -]{0,12}",
            name in "[a-zRé._ -]{0,16}",
        ) {
            prop_assert_eq!(score(&query, &name, TYPO_MIN_QUERY_LEN), score(&query, &name, TYPO_MIN_QUERY_LEN));
        }
    }
}
