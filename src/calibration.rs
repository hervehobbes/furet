use crate::clock::Timestamp;
use tracing::debug;

/// Seconds within which a next visit after a jump signals a probable failure.
pub const FAILURE_THRESHOLD_SECS: i64 = 10;

/// One journaled `queries` row (SPEC section 15).
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
pub struct VisitRecord {
    pub dir_id: i64,
    pub ts: Timestamp,
    pub source: String,
    pub session: String,
}

/// Why a jump was flagged as a probable failure.
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

/// Flags every `queries` jump whose landing was likely a mistake (SPEC section 15).
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
        .filter(|query| query.outcome == "jump")
        .filter_map(|query| {
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
                return Some(ProbableFailure {
                    query,
                    reason: FailureReason::Backtrack,
                });
            }
            if next.dir_id != dir_id {
                return Some(ProbableFailure {
                    query,
                    reason: FailureReason::MovedElsewhere,
                });
            }
            None
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{FailureReason, QueryRecord, VisitRecord, probable_failures};
    use crate::clock::Timestamp;

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
}
