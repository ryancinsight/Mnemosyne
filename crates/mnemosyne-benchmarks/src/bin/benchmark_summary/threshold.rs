pub fn get_regression_threshold(benchmark: &str) -> f64 {
    match benchmark {
        "allocator cycle latency/mnemosyne/small_32" => 1.05,
        "allocator cycle latency/mnemosyne/medium_1024" => 1.05,
        "allocator cycle latency/mnemosyne/large_8192" => 1.05,
        "allocator burst retention/mnemosyne/small_32" => 1.10,
        "cross-thread free handoff/mnemosyne/small_32" => 1.15,
        "threaded saturated small allocation cycles/mnemosyne" => 1.25,
        "segment cache eviction/mnemosyne" => 1.15,
        _ => 1.15,
    }
}

pub fn variance_threshold(benchmark: &str) -> f64 {
    if benchmark.starts_with("threaded small allocation cycles/")
        || benchmark.starts_with("threaded medium allocation cycles/")
        || benchmark.starts_with("threaded saturated small allocation cycles/")
        || benchmark.starts_with("cross-thread free handoff/")
    {
        0.25
    } else {
        0.15
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BASELINE_BENCHMARKS;

    #[test]
    fn saturated_threaded_row_is_the_gated_threaded_baseline() {
        assert!(
            BASELINE_BENCHMARKS.contains(&"threaded saturated small allocation cycles/mnemosyne")
        );
        assert!(
            !BASELINE_BENCHMARKS.contains(&"threaded small allocation cycles/mnemosyne"),
            "scheduler-sensitive historical threaded row must not be threshold-gated"
        );
        assert_eq!(
            get_regression_threshold("threaded saturated small allocation cycles/mnemosyne"),
            1.25
        );
    }

    #[test]
    fn variance_threshold_is_wider_for_threaded_rows() {
        assert_eq!(
            variance_threshold("threaded medium allocation cycles/mnemosyne"),
            0.25
        );
        assert_eq!(
            variance_threshold("threaded saturated small allocation cycles/mnemosyne"),
            0.25
        );
        assert_eq!(
            variance_threshold("allocator cycle latency/mnemosyne/small_32"),
            0.15
        );
    }
}
