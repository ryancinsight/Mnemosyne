//! Repeat-run agreement check for the threshold-gated rows.
//!
//! A regression gate can only distinguish a regression from a rerun when the
//! measurement's run-to-run spread sits below the ceiling it trips at. This
//! module measures exactly that: given the Criterion output roots of several
//! identical runs, it reports each gated row's spread
//! `(max - min) / min` against that row's own ceiling, taken from the
//! [`GATE_ROWS`](crate::config::GATE_ROWS) table the gate itself reads. It is
//! the acceptance oracle for a measurement procedure, and the precondition for
//! refreshing `benchmarks/allocator_baseline_excerpt.csv`.
//!
//! The ceilings are the gate's own: a row gated at `1.05` may not move more
//! than five percent between identical runs, or a rerun alone can trip it.

use crate::config::{GATE_ROWS, GateRow};
use crate::criterion::collect_estimates;
use crate::report::SummaryRow;
use std::io;
use std::path::Path;

/// Fewest runs from which a spread can be computed at all.
const MINIMUM_RUNS: usize = 2;

/// One gated row's agreement across the repeated runs.
struct RowAgreement {
    name: &'static str,
    ceiling: f64,
    /// Point estimates in run order; empty when the row was absent from a run.
    values: Vec<f64>,
}

impl RowAgreement {
    /// Spread as a fraction of the smallest observation, or `None` when the row
    /// did not appear in every run.
    fn spread(&self) -> Option<f64> {
        let low = self.values.iter().copied().reduce(f64::min)?;
        let high = self.values.iter().copied().reduce(f64::max)?;
        (low > 0.0).then(|| (high - low) / low)
    }
}

/// Reads every run's estimates and reports gated-row agreement.
///
/// Returns `Ok(())` when every gated row appeared in every run and stayed
/// within its own ceiling; otherwise an error naming the rows that failed, so
/// the check is usable as a gate in its own right.
///
/// # Errors
///
/// Returns an error when fewer than [`MINIMUM_RUNS`] roots are supplied, when a
/// root cannot be read, or when any gated row is missing or over its ceiling.
pub fn check_repeat_agreement(roots: &[String]) -> io::Result<()> {
    if roots.len() < MINIMUM_RUNS {
        return Err(io::Error::other(format!(
            "repeat agreement needs at least {MINIMUM_RUNS} Criterion roots, got {}",
            roots.len()
        )));
    }

    let mut runs = Vec::with_capacity(roots.len());
    for root in roots {
        let mut rows = Vec::new();
        collect_estimates(Path::new(root), &mut rows)?;
        runs.push(rows);
    }

    let agreements: Vec<RowAgreement> = GATE_ROWS.iter().map(|row| agreement(row, &runs)).collect();

    println!(
        "repeat agreement over {} runs ({} gated rows)",
        roots.len(),
        agreements.len()
    );
    for (index, root) in roots.iter().enumerate() {
        println!("  run {}: {root}", index + 1);
    }

    let mut failures = Vec::new();
    for row in &agreements {
        let Some(spread) = row.spread() else {
            println!("  MISSING  {}", row.name);
            failures.push(row.name);
            continue;
        };
        let over = spread > row.ceiling;
        println!(
            "  {:<7}  spread {:6.2}%  ceiling {:5.2}%  {}",
            if over { "BREACH" } else { "ok" },
            spread * 100.0,
            row.ceiling * 100.0,
            row.name
        );
        if over {
            failures.push(row.name);
        }
    }

    if failures.is_empty() {
        println!("every gated row agrees within its own regression ceiling");
        return Ok(());
    }
    Err(io::Error::other(format!(
        "gated rows exceeded their ceilings across identical runs: {}",
        failures.join(", ")
    )))
}

fn agreement(row: &'static GateRow, runs: &[Vec<SummaryRow<'static>>]) -> RowAgreement {
    let mut values = Vec::with_capacity(runs.len());
    for run in runs {
        let Some(found) = run.iter().find(|candidate| candidate.benchmark == row.name) else {
            values.clear();
            break;
        };
        values.push(found.median_ns);
    }
    RowAgreement {
        name: row.name,
        // A ceiling of 1.05 permits a five percent move.
        ceiling: row.regression_threshold.ratio() - 1.0,
        values,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_is_relative_to_the_smallest_observation() {
        let row = RowAgreement {
            name: "example",
            ceiling: 0.05,
            values: vec![100.0, 110.0, 105.0],
        };

        let spread = row.spread().expect("three observations yield a spread");
        assert!(
            (spread - 0.10).abs() < 1e-12,
            "expected 10% spread, got {spread}"
        );
    }

    #[test]
    fn a_row_absent_from_a_run_has_no_spread() {
        let row = RowAgreement {
            name: "example",
            ceiling: 0.05,
            values: Vec::new(),
        };

        assert!(row.spread().is_none());
    }

    #[test]
    fn fewer_than_two_roots_is_rejected() {
        let single = [String::from("target/criterion")];
        assert!(check_repeat_agreement(&single).is_err());
    }

    /// Every gated row's ceiling is the gate's own threshold expressed as a
    /// permitted move, so tightening a threshold automatically tightens the
    /// agreement this check demands.
    #[test]
    fn ceilings_track_the_gate_thresholds() {
        let runs: [Vec<SummaryRow<'static>>; 2] = [Vec::new(), Vec::new()];
        for gate in GATE_ROWS {
            let row = agreement(
                GATE_ROWS
                    .iter()
                    .find(|candidate| candidate.name == gate.name)
                    .expect("gate row is drawn from GATE_ROWS"),
                &runs,
            );
            assert!(
                (row.ceiling - (gate.regression_threshold.ratio() - 1.0)).abs() < 1e-12,
                "ceiling for {} drifted from its regression threshold",
                gate.name
            );
        }
    }
}
