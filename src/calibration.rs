use crate::clock::Timestamp;
use tracing::debug;

/// Seconds within which a next visit after a jump signals a probable failure.
pub const FAILURE_THRESHOLD_SECS: i64 = 10;

/// One journaled `queries` row (SPEC section 15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryRecord {
    pub id: i64,
    pub ts: Timestamp,
    pub cwd: String,
    pub query: String,
    pub result_dir_id: Option<i64>,
    pub stage: String,
    pub outcome: String,
}

/// One `visits` row, as needed to correlate it with a query (SPEC section 15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisitRecord {
    pub dir_id: i64,
    pub ts: Timestamp,
    pub source: String,
    pub session: String,
}

/// Why a jump was flagged as a probable failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    Backtrack,
    MovedElsewhere,
}

/// A query whose jump was likely a mistake, and why (SPEC section 15).
pub struct ProbableFailure<'a> {
    pub query: &'a QueryRecord,
    pub reason: FailureReason,
}

fn earliest(visits: &[VisitRecord], predicate: impl Fn(&VisitRecord) -> bool) -> Option<usize> {
    visits
        .iter()
        .enumerate()
        .filter(|(_, visit)| predicate(visit))
        .min_by_key(|(index, visit)| (visit.ts, *index))
        .map(|(index, _)| index)
}

/// Why one `queries` row's jump or pick was likely a mistake, if it was; it
/// reads only `visits` with `ts >= query.ts` (SPEC section 15).
pub fn failure_of(query: &QueryRecord, visits: &[VisitRecord]) -> Option<FailureReason> {
    if query.outcome != "jump" && query.outcome != "pick" {
        return None;
    }
    let dir_id = query.result_dir_id?;
    let landing_index = earliest(visits, |visit| {
        visit.dir_id == dir_id && visit.ts >= query.ts
    })?;
    let landing = &visits[landing_index];
    let next_index = earliest(visits, |visit| {
        visit.session == landing.session && visit.ts > landing.ts
    })?;
    let next = &visits[next_index];
    let elapsed = next.ts.unix_seconds() - landing.ts.unix_seconds();
    if elapsed > FAILURE_THRESHOLD_SECS {
        return None;
    }
    if next.source == "back" {
        return Some(FailureReason::Backtrack);
    }
    if next.dir_id != dir_id {
        return Some(FailureReason::MovedElsewhere);
    }
    None
}

/// Flags every `queries` jump or pick whose landing was likely a mistake
/// (SPEC section 15, SPEC-v2 section 24).
pub fn probable_failures<'a>(
    queries: &'a [QueryRecord],
    visits: &[VisitRecord],
) -> Vec<ProbableFailure<'a>> {
    debug!(
        queries = queries.len(),
        visits = visits.len(),
        "probable-failure calibration"
    );
    queries
        .iter()
        .filter_map(|query| {
            failure_of(query, visits).map(|reason| ProbableFailure { query, reason })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{FailureReason, QueryRecord, VisitRecord, failure_of, probable_failures};
    use crate::clock::Timestamp;
    use proptest::prelude::*;

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_unix_seconds(seconds)
    }

    fn query(id: i64, ts: i64, result_dir_id: Option<i64>, outcome: &str) -> QueryRecord {
        QueryRecord {
            id,
            ts: at(ts),
            cwd: "c:\\dev".to_owned(),
            query: "tok".to_owned(),
            result_dir_id,
            stage: "1".to_owned(),
            outcome: outcome.to_owned(),
        }
    }

    fn visit(dir_id: i64, ts: i64, source: &str, session: &str) -> VisitRecord {
        VisitRecord {
            dir_id,
            ts: at(ts),
            source: source.to_owned(),
            session: session.to_owned(),
        }
    }

    #[test]
    fn a_landing_followed_by_back_within_threshold_is_a_backtrack() {
        let queries = [query(1, 100, Some(7), "jump")];
        let visits = [visit(7, 105, "jump", "s"), visit(9, 110, "back", "s")];
        let flagged = probable_failures(&queries, &visits);
        assert_eq!(flagged.len(), 1);
        assert!(matches!(flagged[0].reason, FailureReason::Backtrack));
    }

    #[test]
    fn a_landing_followed_by_a_different_dir_within_threshold_is_moved_elsewhere() {
        let queries = [query(1, 100, Some(7), "jump")];
        let visits = [visit(7, 105, "jump", "s"), visit(9, 110, "hook", "s")];
        let flagged = probable_failures(&queries, &visits);
        assert_eq!(flagged.len(), 1);
        assert!(matches!(flagged[0].reason, FailureReason::MovedElsewhere));
    }

    #[test]
    fn a_landing_followed_by_a_back_to_a_different_dir_is_flagged() {
        let queries = [query(1, 100, Some(7), "jump")];
        let visits = [visit(7, 105, "jump", "s"), visit(9, 110, "back", "s")];
        let flagged = probable_failures(&queries, &visits);
        assert_eq!(flagged.len(), 1);
    }

    #[test]
    fn a_landing_followed_by_the_same_dir_again_is_not_flagged() {
        let queries = [query(1, 100, Some(7), "jump")];
        let visits = [visit(7, 105, "jump", "s"), visit(7, 110, "hook", "s")];
        assert!(probable_failures(&queries, &visits).is_empty());
    }

    #[test]
    fn a_back_at_exactly_eleven_seconds_is_not_flagged() {
        let queries = [query(1, 100, Some(7), "jump")];
        let visits = [visit(7, 100, "jump", "s"), visit(9, 111, "back", "s")];
        assert!(probable_failures(&queries, &visits).is_empty());
    }

    #[test]
    fn a_back_at_exactly_ten_seconds_is_flagged() {
        let queries = [query(1, 100, Some(7), "jump")];
        let visits = [visit(7, 100, "jump", "s"), visit(9, 110, "back", "s")];
        assert_eq!(probable_failures(&queries, &visits).len(), 1);
    }

    #[test]
    fn a_jump_with_no_landing_visit_is_not_flagged() {
        let queries = [query(1, 100, Some(7), "jump")];
        let visits: [VisitRecord; 0] = [];
        assert!(probable_failures(&queries, &visits).is_empty());
    }

    #[test]
    fn a_landing_with_no_further_visits_is_not_flagged() {
        let queries = [query(1, 100, Some(7), "jump")];
        let visits = [visit(7, 105, "jump", "s")];
        assert!(probable_failures(&queries, &visits).is_empty());
    }

    #[test]
    fn a_quick_backtrack_in_a_different_session_is_not_correlated() {
        let queries = [query(1, 100, Some(7), "jump")];
        let visits = [
            visit(7, 105, "jump", "session-a"),
            visit(9, 108, "back", "session-b"),
        ];
        assert!(probable_failures(&queries, &visits).is_empty());
    }

    #[test]
    fn a_non_jump_outcome_is_never_considered() {
        let queries = [query(1, 100, Some(7), "menu")];
        let visits = [visit(7, 105, "jump", "s"), visit(9, 108, "back", "s")];
        assert!(probable_failures(&queries, &visits).is_empty());
    }

    #[test]
    fn a_pick_followed_by_a_quick_back_is_flagged_like_a_jump() {
        let queries = [query(1, 100, Some(7), "pick")];
        let visits = [visit(7, 105, "jump", "s"), visit(9, 110, "back", "s")];
        let flagged = probable_failures(&queries, &visits);
        assert_eq!(flagged.len(), 1);
        assert!(matches!(flagged[0].reason, FailureReason::Backtrack));
    }

    #[test]
    fn a_menu_outcome_is_still_never_considered() {
        let queries = [query(1, 100, Some(7), "menu")];
        let visits = [visit(7, 105, "jump", "s"), visit(9, 110, "back", "s")];
        assert!(probable_failures(&queries, &visits).is_empty());
    }

    #[test]
    fn two_independent_queries_are_each_resolved_without_cross_contamination() {
        let queries = [
            query(1, 100, Some(7), "jump"),
            query(2, 200, Some(8), "jump"),
        ];
        let visits = [
            visit(7, 105, "jump", "session-a"),
            visit(9, 108, "back", "session-a"),
            visit(8, 205, "jump", "session-b"),
            visit(8, 260, "hook", "session-b"),
        ];
        let flagged = probable_failures(&queries, &visits);
        assert_eq!(flagged.len(), 1);
        assert_eq!(flagged[0].query.id, 1);
    }

    fn fixtures() -> Vec<(Vec<QueryRecord>, Vec<VisitRecord>)> {
        vec![
            (
                vec![query(1, 100, Some(7), "jump")],
                vec![visit(7, 105, "jump", "s"), visit(9, 110, "back", "s")],
            ),
            (
                vec![query(1, 100, Some(7), "jump")],
                vec![visit(7, 105, "jump", "s"), visit(9, 110, "hook", "s")],
            ),
            (
                vec![query(1, 100, Some(7), "jump")],
                vec![visit(7, 105, "jump", "s"), visit(7, 110, "hook", "s")],
            ),
            (
                vec![query(1, 100, Some(7), "jump")],
                vec![visit(7, 100, "jump", "s"), visit(9, 111, "back", "s")],
            ),
            (
                vec![query(1, 100, Some(7), "jump")],
                vec![visit(7, 100, "jump", "s"), visit(9, 110, "back", "s")],
            ),
            (vec![query(1, 100, Some(7), "jump")], vec![]),
            (
                vec![query(1, 100, Some(7), "jump")],
                vec![visit(7, 105, "jump", "s")],
            ),
            (
                vec![query(1, 100, Some(7), "jump")],
                vec![
                    visit(7, 105, "jump", "session-a"),
                    visit(9, 108, "back", "session-b"),
                ],
            ),
            (
                vec![query(1, 100, Some(7), "menu")],
                vec![visit(7, 105, "jump", "s"), visit(9, 108, "back", "s")],
            ),
            (
                vec![query(1, 100, Some(7), "pick")],
                vec![visit(7, 105, "jump", "s"), visit(9, 110, "back", "s")],
            ),
            (
                vec![query(1, 100, Some(7), "menu")],
                vec![visit(7, 105, "jump", "s"), visit(9, 110, "back", "s")],
            ),
            (
                vec![
                    query(1, 100, Some(7), "jump"),
                    query(2, 200, Some(8), "jump"),
                ],
                vec![
                    visit(7, 105, "jump", "session-a"),
                    visit(9, 108, "back", "session-a"),
                    visit(8, 205, "jump", "session-b"),
                    visit(8, 260, "hook", "session-b"),
                ],
            ),
        ]
    }

    fn batch_verdicts(
        queries: &[QueryRecord],
        visits: &[VisitRecord],
    ) -> Vec<Option<FailureReason>> {
        let flagged = probable_failures(queries, visits);
        queries
            .iter()
            .map(|query| {
                flagged
                    .iter()
                    .find(|failure| failure.query.id == query.id)
                    .map(|failure| failure.reason)
            })
            .collect()
    }

    fn since(visits: &[VisitRecord], query: &QueryRecord) -> Vec<VisitRecord> {
        visits
            .iter()
            .filter(|kept| kept.ts >= query.ts)
            .cloned()
            .collect()
    }

    fn assert_agreement(queries: &[QueryRecord], visits: &[VisitRecord]) {
        let expected = batch_verdicts(queries, visits);
        for (query, verdict) in queries.iter().zip(&expected) {
            assert_eq!(failure_of(query, visits), *verdict, "query {}", query.id);
            assert_eq!(
                failure_of(query, &since(visits, query)),
                *verdict,
                "query {} with only the visits since it",
                query.id
            );
        }
    }

    #[test]
    fn failure_of_agrees_with_probable_failures_on_every_fixture() {
        let fixtures = fixtures();
        let flagged: usize = fixtures
            .iter()
            .map(|(queries, visits)| probable_failures(queries, visits).len())
            .sum();
        assert_eq!(
            flagged, 5,
            "the fixture table covers flagged and clean rows"
        );
        for (queries, visits) in &fixtures {
            assert_agreement(queries, visits);
        }
    }

    static OUTCOMES: &[&str] = &["jump", "pick", "menu", "none"];
    static SOURCES: &[&str] = &["hook", "jump", "back", "up"];
    static SESSIONS: &[&str] = &["s", "t"];

    fn a_journal() -> impl Strategy<Value = (Vec<QueryRecord>, Vec<VisitRecord>)> {
        (
            proptest::collection::vec(
                (
                    0i64..40,
                    proptest::option::of(1i64..4),
                    proptest::sample::select(OUTCOMES),
                ),
                0..5,
            ),
            proptest::collection::vec(
                (
                    1i64..4,
                    0i64..60,
                    proptest::sample::select(SOURCES),
                    proptest::sample::select(SESSIONS),
                ),
                0..8,
            ),
        )
            .prop_map(|(drawn_queries, drawn_visits)| {
                let queries = drawn_queries
                    .into_iter()
                    .enumerate()
                    .map(|(index, (ts, dir, outcome))| query(index as i64 + 1, ts, dir, outcome))
                    .collect();
                let mut visits: Vec<VisitRecord> = drawn_visits
                    .into_iter()
                    .map(|(dir, ts, source, session)| visit(dir, ts, source, session))
                    .collect();
                visits.sort_by_key(|visit| visit.ts);
                (queries, visits)
            })
    }

    proptest! {
        #[test]
        fn failure_of_agrees_with_probable_failures_on_any_journal((queries, visits) in a_journal()) {
            let expected = batch_verdicts(&queries, &visits);
            for (query, verdict) in queries.iter().zip(&expected) {
                prop_assert_eq!(failure_of(query, &visits), *verdict);
                prop_assert_eq!(failure_of(query, &since(&visits, query)), *verdict);
            }
        }
    }
}
