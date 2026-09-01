//! Per-column Criterion sample budgets.
//!
//! Two budgets, selected by which allocator a column measures:
//!
//! - The **allocator under test** carries the gate. Its rows are compared
//!   against `benchmarks/allocator_baseline_excerpt.csv` at ceilings from
//!   1.05 to 1.25, so their run-to-run spread has to sit below the tightest of
//!   those. Measured on the MN-464 development host, the smoke budget below
//!   left gated rows spreading 12 to 66 percent across identical runs, because
//!   a 100 ms warm-up sizes the iteration count for the whole row: one
//!   scheduling hiccup inside it mis-sizes every sample that follows. The
//!   gate budget takes a second of warm-up and fifty samples over two seconds,
//!   which brought undisturbed repeat runs to one to two percent.
//! - **Comparator columns** feed `benchmarks/allocator_comparison.md`, which
//!   is a snapshot, not a gate. They keep the smoke budget so the whole suite
//!   stays inside its wall-clock bound.
//!
//! This is a budget decision, not a workload decision: neither profile changes
//! what a row measures, how much work an iteration does, or any threshold. See
//! `benchmarks/allocator_baseline_metadata.md` for the measured evidence.

use core::time::Duration;
use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion};

/// Criterion column name of the allocator this repository gates on.
pub const ALLOCATOR_UNDER_TEST: &str = "Mnemosyne";

/// A Criterion sample budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Profile {
    sample_size: usize,
    warm_up: Duration,
    measurement: Duration,
}

/// Budget for threshold-gated rows: enough warm-up for Criterion to size the
/// iteration count off a settled machine, and enough samples that one
/// disturbed sample cannot carry the point estimate.
const GATE: Profile = Profile {
    sample_size: 50,
    warm_up: Duration::from_secs(1),
    measurement: Duration::from_secs(2),
};

/// Budget for comparator columns, which are reported but never gated.
const COMPARATOR: Profile = Profile {
    sample_size: 10,
    warm_up: Duration::from_millis(100),
    measurement: Duration::from_millis(500),
};

/// The suite-wide default, from which every group starts before
/// [`configure_column`] narrows it per column.
#[must_use]
pub fn default_criterion() -> Criterion {
    Criterion::default()
        .sample_size(COMPARATOR.sample_size)
        .warm_up_time(COMPARATOR.warm_up)
        .measurement_time(COMPARATOR.measurement)
}

/// Applies the budget for `column` to `group`.
///
/// Criterion group configuration is stateful — it applies to every benchmark
/// registered after it — so this is called before *each* column, not once per
/// group, and therefore restores the comparator budget as well as raising the
/// gated one.
pub fn configure_column(group: &mut BenchmarkGroup<'_, WallTime>, column: &str) {
    let profile = if column == ALLOCATOR_UNDER_TEST {
        GATE
    } else {
        COMPARATOR
    };
    group
        .sample_size(profile.sample_size)
        .warm_up_time(profile.warm_up)
        .measurement_time(profile.measurement);
}
