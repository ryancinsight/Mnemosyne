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

pub const BASELINE_BENCHMARKS: [&str; 12] = [
    "allocator cycle latency/mnemosyne/small_32",
    "allocator cycle latency/mnemosyne/medium_1024",
    "allocator cycle latency/mnemosyne/large_8192",
    "allocator burst retention/mnemosyne/small_32",
    "cross-thread free handoff/mnemosyne/small_32",
    "threaded saturated small allocation cycles/mnemosyne",
    "segment cache eviction/mnemosyne",
    // Realloc latency rows gate the four Phase 4 buckets
    // {within_class, cross_class, 8k→16k, huge_shrink}. The first
    // two within_class entries and the first two cross_class entries
    // share bucket labels but distinct size ranges; the named 8k→16k
    // and huge_shrink buckets are tracked individually because they
    // sit at size-class boundaries and on the huge-mapping path.
    "realloc latency/mnemosyne/within_class_24_to_32",
    "realloc latency/mnemosyne/cross_class_32_to_64",
    "realloc latency/mnemosyne/within_class_6k_to_8k",
    "realloc latency/mnemosyne/cross_class_8k_to_16k",
    "realloc latency/mnemosyne/huge_shrink_4m_to_2m",
];

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

    /// Every row in [`BASELINE_BENCHMARKS`] must match [`is_active_benchmark`]
    /// — otherwise the gate would silently exclude it from the comparison
    /// summary. The five Phase 4 realloc rows are the new ones this guard
    /// pins so a typo in the literal (e.g., `cross_class` vs `cross-class`)
    /// is caught at `cargo test` time, not at the Criterion run.
    #[test]
    fn baseline_benchmarks_are_all_active_benchmark_rows() {
        for row in BASELINE_BENCHMARKS {
            assert!(
                is_active_benchmark(row),
                "BASELINE_BENCHMARKS row {row:?} is not matched by ACTIVE_GROUPS; \
                 --enforce-thresholds would silently drop it from the gate"
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
                BASELINE_BENCHMARKS.contains(&row),
                "BASELINE_BENCHMARKS is missing Phase 4 gate row {row:?}"
            );
        }
    }
}
