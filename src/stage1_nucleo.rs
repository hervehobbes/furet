use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::normalize::Normalized;
use crate::stage1::{self, SCORE_FLOOR};

/// One query token's nucleo score, after §7.1 normalization of the token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenScore {
    pub token: String,
    pub score: u32,
}

/// Every component behind a nucleo stage-1 score, plus the score itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakdown {
    pub tokens: Vec<TokenScore>,
    pub sum: u32,
    pub floored: u32,
    pub folder_bonus: u32,
    pub total: u32,
}

/// Stage-1 scorer backed by nucleo; one instance reuses one `Matcher`
/// allocation across every candidate it scores.
pub struct NucleoScorer {
    matcher: Matcher,
    haystack: Vec<char>,
}

impl Default for NucleoScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl NucleoScorer {
    /// Builds the scorer and its single `Matcher` with nucleo's default config.
    pub fn new() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            haystack: Vec::new(),
        }
    }

    /// Stage-1 score of `query` against a candidate `name` and its joined
    /// parent segments; `None` when a token does not match the name.
    pub fn score(&mut self, query: &str, name: &str, folder: Option<&str>) -> Option<u32> {
        self.explain(query, name, folder)
            .map(|breakdown| breakdown.total)
    }

    /// Same match as `score`, reporting each token's nucleo score.
    pub fn explain(&mut self, query: &str, name: &str, folder: Option<&str>) -> Option<Breakdown> {
        let tokens: Vec<Normalized> = query.split_whitespace().map(Normalized::new).collect();
        if tokens.is_empty() {
            return None;
        }
        let haystack = Utf32Str::new(name, &mut self.haystack);
        let mut reported: Vec<TokenScore> = Vec::with_capacity(tokens.len());
        for token in &tokens {
            if token.is_empty() {
                return None;
            }
            let text = token.text();
            let atom = Atom::new(
                &text,
                CaseMatching::Ignore,
                Normalization::Smart,
                AtomKind::Fuzzy,
                false,
            );
            let score = u32::from(atom.score(haystack, &mut self.matcher)?);
            reported.push(TokenScore { token: text, score });
        }
        let sum = reported
            .iter()
            .fold(0u32, |total, token| total.saturating_add(token.score));
        let folder_bonus = stage1::folder_bonus(&tokens, folder);
        Some(Breakdown {
            tokens: reported,
            sum,
            floored: sum.max(SCORE_FLOOR),
            folder_bonus,
            total: floored_total(sum, folder_bonus),
        })
    }
}

fn floored_total(sum: u32, folder_bonus: u32) -> u32 {
    sum.max(SCORE_FLOOR).saturating_add(folder_bonus)
}

#[cfg(test)]
mod tests {
    use super::{NucleoScorer, floored_total};
    use crate::stage1::{self, SCORE_FLOOR};
    use crate::stage2;
    use proptest::prelude::*;

    fn score(query: &str, name: &str, folder: Option<&str>) -> Option<u32> {
        NucleoScorer::new().score(query, name, folder)
    }

    #[test]
    fn a_token_missing_from_the_name_fails_the_whole_query() {
        assert!(score("neo vim", "neovim", None).is_some());
        assert_eq!(score("neo zig", "neovim", None), None);
        assert_eq!(score("zig", "neovim", None), None);
        assert_eq!(score("", "neovim", None), None);
        assert_eq!(score("   ", "neovim", None), None);
    }

    #[test]
    fn pattern_syntax_characters_are_matched_literally() {
        assert_eq!(score("^a", "abc", None), None);
        assert!(score("^a", "^abc", None).is_some());
        assert_eq!(score("a$", "ba", None), None);
        assert!(score("a$", "ba$", None).is_some());
        assert_eq!(score("!x", "abc", None), None);
        assert!(score("!x", "a!x", None).is_some());
        assert_eq!(score("'x", "ax", None), None);
        assert!(score("'x", "a'x", None).is_some());
    }

    #[test]
    fn case_is_ignored_on_both_sides() {
        let plain = score("tokio", "tokio", None);
        assert!(plain.is_some());
        assert_eq!(score("TOKIO", "tokio", None), plain);
        assert_eq!(score("tokio", "TOKIO", None), plain);
    }

    #[test]
    fn an_accented_name_matches_a_plain_query_and_the_reverse() {
        assert!(score("reunions", "Réunions", None).is_some());
        assert!(score("réunions", "Reunions", None).is_some());
        assert_eq!(
            score("réunions", "Reunions", None),
            score("reunions", "Reunions", None)
        );
        assert_eq!(
            score("RÉUNIONS", "Réunions", None),
            score("reunions", "Réunions", None)
        );
    }

    #[test]
    fn the_folder_bonus_is_added_after_the_floor() {
        assert_eq!(floored_total(0, 0), SCORE_FLOOR);
        assert_eq!(floored_total(0, 2), SCORE_FLOOR + 2);
        assert_eq!(floored_total(3, 2), SCORE_FLOOR + 2);
        assert_eq!(floored_total(40, 2), 42);
        let plain = score("tk", "tokei", Some("dev\\helix")).expect("the token matches");
        assert_eq!(score("tk", "tokei", None), Some(plain));
        assert_eq!(score("tk", "tokei", Some("dev\\toolkit")), Some(plain + 2));
    }

    #[test]
    fn a_greek_name_with_a_tonos_is_not_matched_even_by_its_own_spelling() {
        assert_eq!(score("αθηνα", "Αθήνα", None), None);
        assert_eq!(score("Αθήνα", "Αθήνα", None), None);
        assert!(score("αθηνα", "Αθηνα", None).is_some());
        assert!(stage1::score("αθηνα", "Αθήνα", None).is_some());
    }

    #[test]
    fn a_cyrillic_short_i_name_is_not_matched_even_by_its_own_spelling() {
        assert_eq!(score("й", "й", None), None);
        assert_eq!(score("и", "й", None), None);
        assert!(score("й", "и", None).is_some());
        assert!(stage1::score("й", "й", None).is_some());
    }

    #[test]
    fn an_nfd_decomposed_name_matches_with_a_lower_score_than_its_composed_form() {
        let decomposed = "Re\u{301}unions";
        let composed = score("reunions", "Réunions", None).expect("the composed name matches");
        let found = score("reunions", decomposed, None).expect("the decomposed name matches");
        assert!(found < composed);
        assert_eq!(score("réunions", decomposed, None), Some(found));
    }

    #[test]
    fn sharp_s_folds_to_its_capital_but_never_to_ss() {
        assert!(score("ß", "ẞ", None).is_some());
        assert!(score("ẞ", "ß", None).is_some());
        assert_eq!(score("strasse", "Straße", None), None);
        assert_eq!(score("straße", "Strasse", None), None);
    }

    #[test]
    fn a_dotted_capital_i_name_matches_a_plain_i_query() {
        assert!(score("istanbul", "İstanbul", None).is_some());
        assert!(score("İstanbul", "istanbul", None).is_some());
    }

    #[test]
    fn a_plain_o_query_matches_o_with_stroke_unlike_the_reference() {
        assert!(score("o", "ø", None).is_some());
        assert_eq!(stage1::score("o", "ø", None), None);
    }

    #[test]
    fn explain_reports_each_token_the_sum_and_the_folder_bonus() {
        let mut scorer = NucleoScorer::new();
        let breakdown = scorer
            .explain("Neo VIM", "neovim", Some("dev\\neovim-src"))
            .expect("both tokens match");
        let tokens: Vec<&str> = breakdown
            .tokens
            .iter()
            .map(|token| token.token.as_str())
            .collect();
        assert_eq!(tokens, ["neo", "vim"]);
        let sum: u32 = breakdown.tokens.iter().map(|token| token.score).sum();
        assert_eq!(breakdown.sum, sum);
        assert_eq!(breakdown.floored, sum.max(SCORE_FLOOR));
        assert_eq!(breakdown.folder_bonus, 2);
        assert_eq!(breakdown.total, breakdown.floored + 2);
        assert_eq!(
            Some(breakdown.total),
            scorer.score("Neo VIM", "neovim", Some("dev\\neovim-src"))
        );
    }

    proptest! {
        #[test]
        fn a_nucleo_score_never_falls_below_the_floor_nor_into_stage_two_range(
            query in "[a-zRéÉ^$!'._ -]{0,12}",
            name in "[a-zRéÉ^$!'._ -]{0,16}",
            folder in "[a-zRé._\\\\ -]{0,16}",
        ) {
            let mut scorer = NucleoScorer::new();
            for found in [
                scorer.score(&query, &name, Some(&folder)),
                scorer.score(&query, &name, None),
            ]
            .into_iter()
            .flatten()
            {
                prop_assert!(found >= SCORE_FLOOR);
                prop_assert!(found > stage2::SCORE_CAP);
            }
        }

        #[test]
        fn nucleo_explain_always_agrees_with_score(
            query in "[a-zRéÉ^$!'._ -]{0,12}",
            name in "[a-zRéÉ^$!'._ -]{0,16}",
            folder in "[a-zRé._\\\\ -]{0,16}",
        ) {
            let mut scorer = NucleoScorer::new();
            let breakdown = scorer.explain(&query, &name, Some(&folder));
            prop_assert_eq!(
                breakdown.as_ref().map(|found| found.total),
                scorer.score(&query, &name, Some(&folder))
            );
            prop_assert_eq!(
                breakdown.as_ref().map(|found| found.total),
                NucleoScorer::new().score(&query, &name, Some(&folder))
            );
            if let Some(found) = breakdown {
                prop_assert_eq!(found.tokens.len(), query.split_whitespace().count());
            }
        }
    }
}
