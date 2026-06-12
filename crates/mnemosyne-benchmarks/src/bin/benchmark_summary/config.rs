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

pub const BASELINE_BENCHMARKS: [&str; 7] = [
    "allocator cycle latency/mnemosyne/small_32",
    "allocator cycle latency/mnemosyne/medium_1024",
    "allocator cycle latency/mnemosyne/large_8192",
    "allocator burst retention/mnemosyne/small_32",
    "cross-thread free handoff/mnemosyne/small_32",
    "threaded saturated small allocation cycles/mnemosyne",
    "segment cache eviction/mnemosyne",
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
}
