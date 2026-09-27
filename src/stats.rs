/// The aggregated database counts printed by `furet stats` (SPEC-v2 §22).
pub struct Counts {
    /// Rows in `dirs` without a `missing_since`.
    pub known_directories: i64,
    /// Rows in `dirs` with a `missing_since`.
    pub missing_directories: i64,
    /// Total rows in `visits`.
    pub visits: i64,
    /// Rows in `visits` inside the 30-day window.
    pub visits_last_30_days: i64,
    /// Total rows in `queries`.
    pub queries: i64,
    /// Rows in `queries` inside the 30-day window.
    pub queries_last_30_days: i64,
    /// Rows in `queries` whose `outcome` is `jump` or `pick`.
    pub jumps: i64,
    /// Rows in `queries` with `stage = '1'`.
    pub stage_1: i64,
    /// Rows in `queries` with `stage = '2'`.
    pub stage_2: i64,
    /// Rows in `queries` with `stage = 'fallback'`.
    pub stage_fallback: i64,
    /// Rows in `queries` with `stage = 'menu'`.
    pub stage_menu: i64,
    /// Rows in `visits` with `source = 'hook'`.
    pub source_hook: i64,
    /// Rows in `visits` with `source = 'jump'`.
    pub source_jump: i64,
    /// Rows in `visits` with `source = 'back'`.
    pub source_back: i64,
    /// Rows in `visits` with `source = 'up'`.
    pub source_up: i64,
    /// Rows in `visits` with `source = 'fallback'`.
    pub source_fallback: i64,
    /// Rows in `visits` with `source = 'import'`.
    pub source_import: i64,
}

/// Renders the report as `key<TAB>value` lines in SPEC-v2 §22's fixed
/// order, then the `top<TAB><visits><TAB><path>` lines, if any.
pub fn render(counts: &Counts, probable_failures: usize, top: &[(i64, String)]) -> Vec<String> {
    let mut lines = vec![
        format!("known_directories\t{}", counts.known_directories),
        format!("missing_directories\t{}", counts.missing_directories),
        format!("visits\t{}", counts.visits),
        format!("visits_last_30_days\t{}", counts.visits_last_30_days),
        format!("queries\t{}", counts.queries),
        format!("queries_last_30_days\t{}", counts.queries_last_30_days),
        format!("jumps\t{}", counts.jumps),
        format!("probable_failures\t{probable_failures}"),
        format!(
            "failure_rate\t{}",
            failure_rate(probable_failures, counts.jumps)
        ),
        format!("stage_1\t{}", counts.stage_1),
        format!("stage_2\t{}", counts.stage_2),
        format!("stage_fallback\t{}", counts.stage_fallback),
        format!("stage_menu\t{}", counts.stage_menu),
        format!("source_hook\t{}", counts.source_hook),
        format!("source_jump\t{}", counts.source_jump),
        format!("source_back\t{}", counts.source_back),
        format!("source_up\t{}", counts.source_up),
        format!("source_fallback\t{}", counts.source_fallback),
        format!("source_import\t{}", counts.source_import),
    ];
    lines.extend(
        top.iter()
            .map(|(visits, path)| format!("top\t{visits}\t{path}")),
    );
    lines
}

fn failure_rate(probable_failures: usize, jumps: i64) -> String {
    if jumps == 0 {
        return "0.0%".to_owned();
    }
    format!("{:.1}%", probable_failures as f64 * 100.0 / jumps as f64)
}

#[cfg(test)]
mod tests {
    use super::{Counts, render};

    fn counts(jumps: i64) -> Counts {
        Counts {
            known_directories: 11,
            missing_directories: 22,
            visits: 33,
            visits_last_30_days: 44,
            queries: 55,
            queries_last_30_days: 66,
            jumps,
            stage_1: 77,
            stage_2: 88,
            stage_fallback: 99,
            stage_menu: 111,
            source_hook: 122,
            source_jump: 133,
            source_back: 144,
            source_up: 155,
            source_fallback: 166,
            source_import: 177,
        }
    }

    #[test]
    fn render_prints_every_key_in_the_fixed_order() {
        let lines = render(&counts(0), 0, &[]);
        let expected: Vec<String> = [
            "known_directories\t11",
            "missing_directories\t22",
            "visits\t33",
            "visits_last_30_days\t44",
            "queries\t55",
            "queries_last_30_days\t66",
            "jumps\t0",
            "probable_failures\t0",
            "failure_rate\t0.0%",
            "stage_1\t77",
            "stage_2\t88",
            "stage_fallback\t99",
            "stage_menu\t111",
            "source_hook\t122",
            "source_jump\t133",
            "source_back\t144",
            "source_up\t155",
            "source_fallback\t166",
            "source_import\t177",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        assert_eq!(lines, expected);
    }

    #[test]
    fn failure_rate_is_zero_without_jumps() {
        let lines = render(&counts(0), 3, &[]);
        assert_eq!(lines[8], "failure_rate\t0.0%");
    }

    #[test]
    fn failure_rate_rounds_to_one_decimal() {
        assert_eq!(render(&counts(3), 1, &[])[8], "failure_rate\t33.3%");
        assert_eq!(render(&counts(3), 2, &[])[8], "failure_rate\t66.7%");
        assert_eq!(render(&counts(8), 1, &[])[8], "failure_rate\t12.5%");
    }

    #[test]
    fn render_appends_top_lines_after_the_statistics() {
        let top = vec![
            (4, "C:\\dev\\helix".to_owned()),
            (0, "C:\\dev\\quiet".to_owned()),
        ];
        let lines = render(&counts(0), 0, &top);
        assert_eq!(lines.len(), 21);
        assert_eq!(lines[19], "top\t4\tC:\\dev\\helix");
        assert_eq!(lines[20], "top\t0\tC:\\dev\\quiet");
    }
}
