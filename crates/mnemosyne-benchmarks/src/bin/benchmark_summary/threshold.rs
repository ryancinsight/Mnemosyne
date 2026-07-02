use crate::config::{gate_row, RegressionThreshold, VarianceThreshold};

/// Resolves the regression `mean_ratio` ceiling for a benchmark row.
///
/// Gated rows carry an explicit ceiling in the [`crate::config::GATE_ROWS`]
/// SSOT table; every other row falls back to [`RegressionThreshold::DEFAULT`].
pub fn get_regression_threshold(benchmark: &str) -> f64 {
    gate_row(benchmark)
        .map(|row| row.regression_threshold)
        .unwrap_or(RegressionThreshold::DEFAULT)
        .ratio()
}

/// Resolves the variance (noise) ceiling for a benchmark row.
///
/// Gated rows carry their ceiling in the [`crate::config::GATE_ROWS`] SSOT
/// table. Rows not in the table (the variance report covers every row, not
/// only the gated ones) are classified by name prefix: scheduler-sensitive
/// threaded and cross-thread groups get the wider [`VarianceThreshold::THREADED`]
/// ceiling, all others the [`VarianceThreshold::DEFAULT`].
pub fn variance_threshold(benchmark: &str) -> f64 {
    if let Some(row) = gate_row(benchmark) {
        return row.variance_threshold.ratio();
    }
    variance_class_for(benchmark).ratio()
}

/// Prefix classifier for the variance ceiling of a non-gated row.
pub(crate) fn variance_class_for(benchmark: &str) -> VarianceThreshold {
    if benchmark.starts_with("threaded small allocation cycles/")
        || benchmark.starts_with("threaded medium allocation cycles/")
        || benchmark.starts_with("threaded saturated small allocation cycles/")
        || benchmark.starts_with("cross-thread free handoff/")
    {
        VarianceThreshold::THREADED
    } else {
        VarianceThreshold::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::gate_row;

    #[test]
    fn saturated_threaded_row_is_the_gated_threaded_baseline() {
        assert!(gate_row("threaded saturated small allocation cycles/mnemosyne").is_some());
        assert!(
            gate_row("threaded small allocation cycles/mnemosyne").is_none(),
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
