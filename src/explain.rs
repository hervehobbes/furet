use crate::decision::{self, Decision};
use crate::normalize::Normalized;
use crate::rank::{self, Candidate, Stage, TieBreak};
use crate::{stage1, stage2};

/// Why a stored directory never reached the ranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elimination {
    CurrentDirectory,
    Disappeared,
    NoSubsequence,
    DistanceTooFar { distance: usize },
}

/// What this query did with one stored directory: match it with its score
/// detail, or drop it with a reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evaluation<'a> {
    Matched {
        candidate: &'a Candidate,
        stage: Stage,
        score: u32,
        stage1: Option<stage1::Breakdown>,
        stage2_distance: Option<usize>,
    },
    Eliminated {
        candidate: &'a Candidate,
        reason: Elimination,
    },
}

/// Where a report's candidates came from (SPEC section 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Database,
    Fallback,
}

/// Everything SPEC section 14 reports about one query; no jump is performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report<'a> {
    pub normalized_query: String,
    /// Set by `query --local`: the git project root the pool was scoped to.
    pub project_root: Option<String>,
    pub evaluations: Vec<Evaluation<'a>>,
    pub decision: Decision<'a>,
    pub deciding_criterion: Option<TieBreak>,
    pub origin: Origin,
    pub stage2_max_distance: usize,
}

/// Ranks and decides exactly as `furet query` would, keeping the score
/// detail of every candidate and the reason behind every exclusion.
pub fn explain<'a>(
    query: &str,
    current_dir: &str,
    candidates: &'a [Candidate],
    origin: Origin,
    typo_min_length: usize,
) -> Report<'a> {
    let ranked = rank::rank(query, current_dir, candidates, typo_min_length);
    let mut evaluations: Vec<Evaluation<'a>> = Vec::with_capacity(candidates.len());
    for scored in &ranked {
        let name = &scored.candidate.name;
        evaluations.push(Evaluation::Matched {
            candidate: scored.candidate,
            stage: scored.stage,
            score: scored.score,
            stage1: match scored.stage {
                Stage::One => stage1::explain(query, name, scored.candidate.folder.as_deref()),
                Stage::Two => None,
            },
            stage2_distance: match scored.stage {
                Stage::One => None,
                Stage::Two => stage2::explain(query, name, typo_min_length),
            },
        });
    }
    let mut dropped: Vec<&'a Candidate> = candidates
        .iter()
        .filter(|candidate| {
            !ranked
                .iter()
                .any(|scored| std::ptr::eq(scored.candidate, *candidate))
        })
        .collect();
    dropped.sort_by(|left, right| left.path.cmp(&right.path));
    for candidate in dropped {
        evaluations.push(Evaluation::Eliminated {
            candidate,
            reason: eliminate(query, current_dir, candidate, typo_min_length),
        });
    }
    Report {
        normalized_query: Normalized::new(query).text(),
        project_root: None,
        decision: decision::decide(&ranked),
        deciding_criterion: rank::deciding_criterion(&ranked),
        evaluations,
        origin,
        stage2_max_distance: stage2::query_max_distance(query),
    }
}

fn eliminate(
    query: &str,
    current_dir: &str,
    candidate: &Candidate,
    typo_min_length: usize,
) -> Elimination {
    if rank::same_path(&candidate.path, current_dir) {
        return Elimination::CurrentDirectory;
    }
    if candidate.missing {
        return Elimination::Disappeared;
    }
    match stage2::explain(query, &candidate.name, typo_min_length) {
        Some(distance) => Elimination::DistanceTooFar { distance },
        None => Elimination::NoSubsequence,
    }
}

/// Renders a report as the plain text `--explain` writes to stderr; pure and
/// stable, so it can be snapshot tested.
pub fn render(report: &Report) -> String {
    let mut rendered = format!("normalized query: {}\n", report.normalized_query);
    if let Some(root) = &report.project_root {
        rendered.push_str(&format!("project root: {root}\n"));
    }
    if report.origin == Origin::Fallback {
        rendered.push_str("origin: fallback\n");
    }
    rendered.push_str("evaluated candidates:\n");
    let mut evaluated = 0usize;
    for evaluation in &report.evaluations {
        if let Evaluation::Matched {
            candidate,
            stage,
            score,
            stage1,
            stage2_distance,
        } = evaluation
        {
            evaluated += 1;
            rendered.push_str(&format!(
                "  {} score {} {}\n",
                stage_label(*stage),
                score,
                candidate.path
            ));
            if let Some(breakdown) = stage1 {
                rendered.push_str(&render_breakdown(breakdown));
            }
            if let Some(distance) = stage2_distance {
                rendered.push_str(&format!(
                    "    distance {distance} (max {})\n",
                    report.stage2_max_distance
                ));
            }
        }
    }
    if evaluated == 0 {
        rendered.push_str("  (none)\n");
    }
    rendered.push_str("eliminated candidates:\n");
    let mut eliminated = 0usize;
    for evaluation in &report.evaluations {
        if let Evaluation::Eliminated { candidate, reason } = evaluation {
            eliminated += 1;
            rendered.push_str(&format!(
                "  {}: {}\n",
                candidate.path,
                reason_label(*reason, report.stage2_max_distance)
            ));
        }
    }
    if eliminated == 0 {
        rendered.push_str("  (none)\n");
    }
    rendered.push_str(&format!(
        "deciding criterion: {}\n",
        criterion_label(report.deciding_criterion)
    ));
    rendered.push_str(&decision_label(&report.decision));
    rendered
}

fn render_breakdown(breakdown: &stage1::Breakdown) -> String {
    let mut rendered = String::new();
    let mut running: i64 = 0;
    for token in &breakdown.tokens {
        let subtotal = token.base + token.length + token.placement + token.prefix + token.density;
        running += subtotal;
        rendered.push_str(&format!(
            "    token '{}': base {} + length {} + placement {} + prefix {} + density {} = {}\n",
            token.token,
            token.base,
            token.length,
            token.placement,
            token.prefix,
            token.density,
            subtotal
        ));
    }
    running += breakdown.order_bonus;
    rendered.push_str(&format!("    order bonus {:+}\n", breakdown.order_bonus));
    if running < i64::from(stage1::SCORE_FLOOR) {
        rendered.push_str(&format!(
            "    floor raises {running} to {}\n",
            stage1::SCORE_FLOOR
        ));
    }
    rendered.push_str(&format!("    folder bonus {:+}\n", breakdown.folder_bonus));
    rendered
}

fn stage_label(stage: Stage) -> &'static str {
    match stage {
        Stage::One => "stage 1",
        Stage::Two => "stage 2",
    }
}

fn reason_label(reason: Elimination, max_distance: usize) -> String {
    match reason {
        Elimination::CurrentDirectory => "current directory".to_owned(),
        Elimination::Disappeared => "disappeared".to_owned(),
        Elimination::NoSubsequence => "no subsequence".to_owned(),
        Elimination::DistanceTooFar { distance } => {
            format!("distance {distance} > {max_distance}")
        }
    }
}

fn criterion_label(criterion: Option<TieBreak>) -> &'static str {
    match criterion {
        None => "none (no runner-up)",
        Some(TieBreak::Score) => "score",
        Some(TieBreak::Recency) => "recency",
        Some(TieBreak::NameLength) => "name length",
        Some(TieBreak::Path) => "path",
    }
}

fn decision_label(decision: &Decision) -> String {
    match decision {
        Decision::None => "decision: none\n".to_owned(),
        Decision::Jump(candidate) => format!("decision: jump {}\n", candidate.path),
        Decision::Menu(shown) => {
            let mut rendered = String::from("decision: menu\n");
            for (index, candidate) in shown.iter().enumerate() {
                rendered.push_str(&format!("  {}) {}\n", index + 1, candidate.path));
            }
            rendered
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Elimination, Evaluation, Origin, Report, explain, render};
    use crate::clock::Timestamp;
    use crate::decision::Decision;
    use crate::rank::{Candidate, Stage, TieBreak};
    use crate::stage2::TYPO_MIN_QUERY_LEN;

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

    fn reason(report: &Report, path: &str) -> Option<Elimination> {
        report
            .evaluations
            .iter()
            .find_map(|evaluation| match evaluation {
                Evaluation::Eliminated { candidate, reason } if candidate.path == path => {
                    Some(*reason)
                }
                _ => None,
            })
    }

    fn matched(report: &Report, path: &str) -> Option<(Stage, u32)> {
        report
            .evaluations
            .iter()
            .find_map(|evaluation| match evaluation {
                Evaluation::Matched {
                    candidate,
                    stage,
                    score,
                    ..
                } if candidate.path == path => Some((*stage, *score)),
                _ => None,
            })
    }

    fn world() -> Vec<Candidate> {
        let mut gone = dir("/dev/gone", "gone", "/dev", 5);
        gone.missing = true;
        vec![
            dir("/dev/tokio", "tokio", "/dev", 1),
            dir("/dev/tokyo", "tokyo", "/dev", 2),
            dir("/dev/zellij", "zellij", "/dev", 3),
            dir("/dev/helix", "helix", "/dev", 4),
            gone,
        ]
    }

    #[test]
    fn every_candidate_is_evaluated_exactly_once() {
        let candidates = world();
        let report = explain(
            "tokio",
            "/dev/helix",
            &candidates,
            Origin::Database,
            TYPO_MIN_QUERY_LEN,
        );
        assert_eq!(report.evaluations.len(), candidates.len());
        assert_eq!(report.normalized_query, "tokio");
    }

    #[test]
    fn the_current_directory_is_eliminated_before_anything_else() {
        let candidates = world();
        let report = explain(
            "helix",
            "/DEV/Helix",
            &candidates,
            Origin::Database,
            TYPO_MIN_QUERY_LEN,
        );
        assert_eq!(
            reason(&report, "/dev/helix"),
            Some(Elimination::CurrentDirectory)
        );
    }

    #[test]
    fn a_missing_candidate_is_eliminated_as_disappeared() {
        let mut candidates = world();
        candidates[0].missing = true;
        let report = explain(
            "tokio",
            "",
            &candidates,
            Origin::Database,
            TYPO_MIN_QUERY_LEN,
        );
        assert_eq!(
            reason(&report, "/dev/tokio"),
            Some(Elimination::Disappeared)
        );
        assert_eq!(reason(&report, "/dev/gone"), Some(Elimination::Disappeared));
    }

    #[test]
    fn a_candidate_missing_and_current_reports_the_current_directory_first() {
        let mut candidates = world();
        candidates[0].missing = true;
        let report = explain(
            "tokio",
            "/dev/tokio",
            &candidates,
            Origin::Database,
            TYPO_MIN_QUERY_LEN,
        );
        assert_eq!(
            reason(&report, "/dev/tokio"),
            Some(Elimination::CurrentDirectory)
        );
    }

    #[test]
    fn a_query_too_short_for_stage_two_eliminates_on_no_subsequence() {
        let candidates = world();
        let report = explain("tok", "", &candidates, Origin::Database, TYPO_MIN_QUERY_LEN);
        assert_eq!(matched(&report, "/dev/tokio"), Some((Stage::One, 68)));
        assert_eq!(
            reason(&report, "/dev/helix"),
            Some(Elimination::NoSubsequence)
        );
        assert_eq!(
            reason(&report, "/dev/zellij"),
            Some(Elimination::NoSubsequence)
        );
    }

    #[test]
    fn a_name_too_far_from_the_query_reports_its_distance() {
        let candidates = world();
        let report = explain(
            "tokio",
            "",
            &candidates,
            Origin::Database,
            TYPO_MIN_QUERY_LEN,
        );
        assert_eq!(
            reason(&report, "/dev/zellij"),
            Some(Elimination::DistanceTooFar { distance: 4 })
        );
        assert_eq!(
            reason(&report, "/dev/helix"),
            Some(Elimination::DistanceTooFar { distance: 4 })
        );
    }

    #[test]
    fn an_elimination_names_the_threshold_the_query_length_applies() {
        let short = [dir("/dev/tokei", "tokei", "/dev", 1)];
        let report = explain("tokio", "", &short, Origin::Database, TYPO_MIN_QUERY_LEN);
        assert_eq!(report.stage2_max_distance, 1);
        assert_eq!(
            reason(&report, "/dev/tokei"),
            Some(Elimination::DistanceTooFar { distance: 2 })
        );
        let rendered = render(&report);
        assert!(
            rendered.contains("/dev/tokei: distance 2 > 1"),
            "{rendered}"
        );
        let long = [dir("/dev/neovim", "neovim", "/dev", 1)];
        let accepted = explain("meovin", "", &long, Origin::Database, TYPO_MIN_QUERY_LEN);
        assert_eq!(accepted.stage2_max_distance, 2);
        let rendered = render(&accepted);
        assert!(rendered.contains("distance 2 (max 2)"), "{rendered}");
    }

    #[test]
    fn stage_one_wins_before_stage_two_and_both_keep_their_detail() {
        let candidates = world();
        let report = explain(
            "tokio",
            "",
            &candidates,
            Origin::Database,
            TYPO_MIN_QUERY_LEN,
        );
        assert_eq!(matched(&report, "/dev/tokio"), Some((Stage::One, 90)));
        assert_eq!(matched(&report, "/dev/tokyo"), Some((Stage::Two, 2)));
        let detail: Vec<(bool, bool)> = report
            .evaluations
            .iter()
            .filter_map(|evaluation| match evaluation {
                Evaluation::Matched {
                    stage1,
                    stage2_distance,
                    ..
                } => Some((stage1.is_some(), stage2_distance.is_some())),
                Evaluation::Eliminated { .. } => None,
            })
            .collect();
        assert_eq!(detail, [(true, false), (false, true)]);
    }

    #[test]
    fn the_decision_and_the_tie_break_match_the_ranking() {
        let candidates = world();
        let report = explain(
            "tokio",
            "",
            &candidates,
            Origin::Database,
            TYPO_MIN_QUERY_LEN,
        );
        assert_eq!(report.decision, Decision::Jump(&candidates[0]));
        assert_eq!(report.deciding_criterion, Some(TieBreak::Score));
        let empty = explain(
            "zigzag",
            "",
            &candidates,
            Origin::Database,
            TYPO_MIN_QUERY_LEN,
        );
        assert_eq!(empty.decision, Decision::None);
        assert_eq!(empty.deciding_criterion, None);
    }

    #[test]
    fn a_stage_two_tie_reports_the_menu_it_would_open() {
        let candidates = [
            dir("/aaa/tokio", "tokio", "/aaa", 1),
            dir("/zzz/tokio", "tokio", "/zzz", 2),
        ];
        let report = explain(
            "tokoi",
            "",
            &candidates,
            Origin::Database,
            TYPO_MIN_QUERY_LEN,
        );
        assert_eq!(
            report.decision,
            Decision::Menu(vec![&candidates[0], &candidates[1]])
        );
        assert_eq!(report.deciding_criterion, Some(TieBreak::Recency));
    }

    #[test]
    fn render_lays_out_every_section_in_a_stable_order() {
        let candidates = world();
        let rendered = render(&explain(
            "tokio",
            "/dev/helix",
            &candidates,
            Origin::Database,
            TYPO_MIN_QUERY_LEN,
        ));
        let expected = "normalized query: tokio\n\
            evaluated candidates:\n\
            \x20 stage 1 score 90 /dev/tokio\n\
            \x20   token 'tokio': base 10 + length 5 + placement 50 + prefix 10 + density 10 = 85\n\
            \x20   order bonus +5\n\
            \x20   folder bonus +0\n\
            \x20 stage 2 score 2 /dev/tokyo\n\
            \x20   distance 1 (max 1)\n\
            eliminated candidates:\n\
            \x20 /dev/gone: disappeared\n\
            \x20 /dev/helix: current directory\n\
            \x20 /dev/zellij: distance 4 > 1\n\
            deciding criterion: score\n\
            decision: jump /dev/tokio\n";
        assert_eq!(rendered, expected);
    }

    #[test]
    fn render_names_the_floor_and_an_empty_report() {
        let long = format!("a{}z", "b".repeat(50));
        let candidates = [dir("/dev/long", &long, "/dev", 1)];
        let rendered = render(&explain(
            "az",
            "",
            &candidates,
            Origin::Database,
            TYPO_MIN_QUERY_LEN,
        ));
        assert!(rendered.contains("floor raises"), "{rendered}");
        assert!(rendered.contains("stage 1 score 4 /dev/long"), "{rendered}");
        let nothing: [Candidate; 0] = [];
        assert_eq!(
            render(&explain(
                "tokio",
                "",
                &nothing,
                Origin::Database,
                TYPO_MIN_QUERY_LEN
            )),
            "normalized query: tokio\nevaluated candidates:\n  (none)\neliminated candidates:\n  (none)\ndeciding criterion: none (no runner-up)\ndecision: none\n"
        );
    }

    #[test]
    fn a_fallback_origin_renders_an_extra_origin_line() {
        let candidates = [dir("/dev/tokio", "tokio", "/dev", 1)];
        let database = render(&explain(
            "tokio",
            "",
            &candidates,
            Origin::Database,
            TYPO_MIN_QUERY_LEN,
        ));
        assert!(!database.contains("origin:"), "{database}");
        let fallback = render(&explain(
            "tokio",
            "",
            &candidates,
            Origin::Fallback,
            TYPO_MIN_QUERY_LEN,
        ));
        insta::assert_snapshot!(fallback);
    }
}
