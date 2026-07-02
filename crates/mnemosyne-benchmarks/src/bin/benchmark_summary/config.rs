pub const CRITERION_ROOT: &str = "target/criterion";
pub const SUMMARY_PATH: &str = "target/criterion/benchmark_summary.csv";
pub const COMPARISON_PATH: &str = "target/criterion/benchmark_baseline_comparison.csv";
pub const CURRENT_EXCERPT_PATH: &str = "target/criterion/allocator_current_excerpt.csv";
pub const VARIANCE_PATH: &str = "target/criterion/benchmark_variance.csv";
pub const METADATA_PATH: &str = "target/criterion/benchmark_metadata.json";
pub const BASELINE_PATH: &str = "benchmarks/allocator_baseline_excerpt.csv";
pub const REFRESH_BASELINE_FLAG: &str = "--refresh-baseline";
pub const ENFORCE_THRESHOLDS_FLAG: &str = "--enforce-thresholds";

pub const ACTIVE_GROUPS: [&str; 12] = [
    "allocator allocation latency/",
    "allocator deallocation latency/",
    "allocator burst retention/",
    "allocator cycle latency/",
    "cross-thread free handoff/",
    "realloc latency/",
    "segment cache eviction/",
    "threaded medium allocation cycles/",
    "threaded small allocation cycles/",
    "threaded saturated small allocation cycles/",
    "usable size query latency/",
    "usable size latency/",
];

/// A threshold-gated baseline benchmark row.
///
/// One [`GateRow`] carries the fully-qualified Criterion row `name`, the
/// per-row `regression_threshold` (the `mean_ratio` ceiling before a run is
/// flagged as a regression), and the `variance_threshold` (the noise ceiling
/// reported for that row). These three values are the single source of truth
/// read by both threshold enforcement ([`super::threshold`]) and the summary
/// gate; do not duplicate any of them elsewhere.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GateRow {
    pub name: &'static str,
    pub regression_threshold: RegressionThreshold,
    pub variance_threshold: VarianceThreshold,
}

/// A regression `mean_ratio` ceiling, stored as thousandths to keep the SSOT
/// table `Eq`/`Hash`-comparable while representing the two-decimal thresholds
/// exactly (`1.05` == `1050`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegressionThreshold(u16);

/// A variance (noise) ceiling, stored as thousandths for the same reason as
/// [`RegressionThreshold`] (`0.15` == `150`, `0.25` == `250`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VarianceThreshold(u16);

impl RegressionThreshold {
    /// The default regression ceiling applied to any row not named in
    /// [`GATE_ROWS`].
    pub const DEFAULT: Self = Self(1150);

    #[inline]
    pub const fn ratio(self) -> f64 {
        self.0 as f64 / 1000.0
    }
}

impl VarianceThreshold {
    /// The variance ceiling for scheduler-sensitive threaded / cross-thread
    /// rows, whose run-to-run noise is inherently wider.
    pub const THREADED: Self = Self(250);
    /// The variance ceiling for all other (latency-class) rows.
    pub const DEFAULT: Self = Self(150);

    #[inline]
    pub const fn ratio(self) -> f64 {
        self.0 as f64 / 1000.0
    }
}

/// Single source of truth for the threshold-gated baseline rows and their
/// per-row regression and variance ceilings.
///
/// Realloc latency rows gate the four Phase 4 buckets
/// {within_class, cross_class, 8k→16k, huge_shrink}. The first two
/// within_class entries and the first two cross_class entries share bucket
/// labels but distinct size ranges; the named 8k→16k and huge_shrink buckets
/// are tracked individually because they sit at size-class boundaries and on
/// the huge-mapping path.
pub const GATE_ROWS: [GateRow; 12] = [
    GateRow {
        name: "allocator cycle latency/mnemosyne/small_32",
        regression_threshold: RegressionThreshold(1050),
        variance_threshold: VarianceThreshold::DEFAULT,
    },
    GateRow {
        name: "allocator cycle latency/mnemosyne/medium_1024",
        regression_threshold: RegressionThreshold(1050),
        variance_threshold: VarianceThreshold::DEFAULT,
    },
    GateRow {
        name: "allocator cycle latency/mnemosyne/large_8192",
        regression_threshold: RegressionThreshold(1050),
        variance_threshold: VarianceThreshold::DEFAULT,
    },
    GateRow {
        name: "allocator burst retention/mnemosyne/small_32",
        regression_threshold: RegressionThreshold(1100),
        variance_threshold: VarianceThreshold::DEFAULT,
    },
    GateRow {
        name: "cross-thread free handoff/mnemosyne/small_32",
        regression_threshold: RegressionThreshold::DEFAULT,
        variance_threshold: VarianceThreshold::THREADED,
    },
    GateRow {
        name: "threaded saturated small allocation cycles/mnemosyne",
        regression_threshold: RegressionThreshold(1250),
        variance_threshold: VarianceThreshold::THREADED,
    },
    GateRow {
        name: "segment cache eviction/mnemosyne",
        regression_threshold: RegressionThreshold::DEFAULT,
        variance_threshold: VarianceThreshold::DEFAULT,
    },
    GateRow {
        name: "realloc latency/mnemosyne/within_class_24_to_32",
        regression_threshold: RegressionThreshold::DEFAULT,
        variance_threshold: VarianceThreshold::DEFAULT,
    },
    GateRow {
        name: "realloc latency/mnemosyne/cross_class_32_to_64",
        regression_threshold: RegressionThreshold::DEFAULT,
        variance_threshold: VarianceThreshold::DEFAULT,
    },
    GateRow {
        name: "realloc latency/mnemosyne/within_class_6k_to_8k",
        regression_threshold: RegressionThreshold::DEFAULT,
        variance_threshold: VarianceThreshold::DEFAULT,
    },
    GateRow {
        name: "realloc latency/mnemosyne/cross_class_8k_to_16k",
        regression_threshold: RegressionThreshold::DEFAULT,
        variance_threshold: VarianceThreshold::DEFAULT,
    },
    GateRow {
        name: "realloc latency/mnemosyne/huge_shrink_4m_to_2m",
        regression_threshold: RegressionThreshold::DEFAULT,
        variance_threshold: VarianceThreshold::DEFAULT,
    },
];

/// Fully-qualified names of the [`GATE_ROWS`], projected for callers that only
/// need the row identifiers (baseline excerpt selection and refresh).
pub fn baseline_benchmarks() -> impl Iterator<Item = &'static str> {
    GATE_ROWS.iter().map(|row| row.name)
}

/// Looks up a gate row by its fully-qualified Criterion name.
pub fn gate_row(benchmark: &str) -> Option<&'static GateRow> {
    GATE_ROWS.iter().find(|row| row.name == benchmark)
}

pub fn is_active_benchmark(benchmark: &str) -> bool {
    ACTIVE_GROUPS
        .iter()
        .any(|group| benchmark.starts_with(group))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_filter_keeps_every_allocator_benchmark_group() {
        const EXPECTED_GROUPS: [&str; 12] = [
            "allocator allocation latency/",
            "allocator deallocation latency/",
            "allocator burst retention/",
            "allocator cycle latency/",
            "cross-thread free handoff/",
            "realloc latency/",
            "segment cache eviction/",
            "threaded medium allocation cycles/",
            "threaded small allocation cycles/",
            "threaded saturated small allocation cycles/",
            "usable size query latency/",
            "usable size latency/",
        ];

        assert_eq!(ACTIVE_GROUPS, EXPECTED_GROUPS);
        for group in EXPECTED_GROUPS {
            let benchmark = format!("{group}mnemosyne/smoke");
            assert!(
                is_active_benchmark(&benchmark),
                "active benchmark filter dropped {benchmark}"
            );
        }
    }

    #[test]
    fn active_filter_rejects_untracked_benchmark_groups() {
        assert!(
            !is_active_benchmark("tls lookup overhead/standardtls"),
            "TLS exploratory benchmark rows must not enter allocator comparison summaries"
        );
    }

    /// Every row in [`GATE_ROWS`] must match [`is_active_benchmark`]
    /// — otherwise the gate would silently exclude it from the comparison
    /// summary. The five Phase 4 realloc rows are the new ones this guard
    /// pins so a typo in the literal (e.g., `cross_class` vs `cross-class`)
    /// is caught at `cargo test` time, not at the Criterion run.
    #[test]
    fn baseline_benchmarks_are_all_active_benchmark_rows() {
        for row in baseline_benchmarks() {
            assert!(
                is_active_benchmark(row),
                "GATE_ROWS row {row:?} is not matched by ACTIVE_GROUPS; \
                 --enforce-thresholds would silently drop it from the gate"
            );
        }
    }

    /// The variance ceiling recorded in each [`GateRow`] must equal what the
    /// independent prefix classifier would assign the same row, so the table
    /// stays a faithful SSOT rather than drifting from the general classifier
    /// that governs every non-gated row in the variance report.
    #[test]
    fn gate_row_variance_matches_prefix_classifier() {
        for row in GATE_ROWS {
            assert_eq!(
                row.variance_threshold,
                crate::threshold::variance_class_for(row.name),
                "GateRow {:?} variance ceiling drifted from the prefix classifier",
                row.name
            );
        }
    }

    /// The regression ceiling recorded in each [`GateRow`] must equal what
    /// [`super::super::threshold::get_regression_threshold`] resolves for the
    /// same row.
    #[test]
    fn gate_row_regression_matches_threshold_lookup() {
        for row in GATE_ROWS {
            assert_eq!(
                row.regression_threshold.ratio(),
                crate::threshold::get_regression_threshold(row.name),
                "GateRow {:?} regression ceiling drifted from the threshold lookup",
                row.name
            );
        }
    }

    /// Phase 4 gate rows: pin the four buckets the renormalization plan
    /// cites — {within_class, cross_class, 8k→16k, huge_shrink}. The
    /// first two buckets are represented by one row each; `8k_to_16k`
    /// and `huge_shrink` are individually tracked because they sit on
    /// size-class boundaries and on the huge-mapping path respectively.
    #[test]
    fn phase_four_realloc_gate_rows_are_tracked() {
        const PHASE_FOUR_GATE_ROWS: [&str; 5] = [
            "realloc latency/mnemosyne/within_class_24_to_32",
            "realloc latency/mnemosyne/cross_class_32_to_64",
            "realloc latency/mnemosyne/within_class_6k_to_8k",
            "realloc latency/mnemosyne/cross_class_8k_to_16k",
            "realloc latency/mnemosyne/huge_shrink_4m_to_2m",
        ];
        for row in PHASE_FOUR_GATE_ROWS {
            assert!(
                gate_row(row).is_some(),
                "GATE_ROWS is missing Phase 4 gate row {row:?}"
            );
        }
    }
}
