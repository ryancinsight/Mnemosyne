#[path = "benchmark_summary/allocator.rs"]
mod allocator;
#[path = "benchmark_summary/config.rs"]
mod config;
#[path = "benchmark_summary/criterion.rs"]
mod criterion;
#[path = "benchmark_summary/csv.rs"]
mod csv;
#[path = "benchmark_summary/metadata.rs"]
mod metadata;
#[path = "benchmark_summary/report.rs"]
mod report;
#[path = "benchmark_summary/threshold.rs"]
mod threshold;

use config::{
    BASELINE_BENCHMARKS, BASELINE_PATH, COMPARISON_PATH, CRITERION_ROOT, CURRENT_EXCERPT_PATH,
    ENFORCE_THRESHOLDS_FLAG, METADATA_PATH, REFRESH_BASELINE_FLAG, SUMMARY_PATH, VARIANCE_PATH,
};
use criterion::collect_estimates;
use report::{
    compare_to_baseline, missing_selected_benchmarks, read_summary, write_comparison,
    write_summary, write_variance_report,
};
use std::fs;
use std::io;
use std::path::Path;
use threshold::get_regression_threshold;

fn main() -> io::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let refresh_baseline = args.iter().any(|arg| arg == REFRESH_BASELINE_FLAG);
    let enforce_thresholds = args.iter().any(|arg| arg == ENFORCE_THRESHOLDS_FLAG);
    let root = Path::new(CRITERION_ROOT);
    let baseline_content = if Path::new(BASELINE_PATH).exists() {
        fs::read_to_string(BASELINE_PATH)?
    } else {
        String::new()
    };
    let previous_baseline = read_summary(&baseline_content)?;
    let mut rows = Vec::new();
    collect_estimates(root, &mut rows)?;
    rows.retain(|row| config::is_active_benchmark(&row.benchmark));
    rows.sort_by(|a, b| a.benchmark.cmp(&b.benchmark));

    write_summary(SUMMARY_PATH, &rows)?;
    write_variance_report(VARIANCE_PATH, &rows)?;
    let comparisons = compare_to_baseline(&previous_baseline, &rows);
    write_comparison(COMPARISON_PATH, &comparisons)?;

    let current_excerpt_rows = BASELINE_BENCHMARKS
        .iter()
        .filter_map(|benchmark| rows.iter().find(|row| row.benchmark == *benchmark))
        .cloned()
        .collect::<Vec<_>>();
    let missing_baseline_rows = missing_selected_benchmarks(&rows);
    write_summary(CURRENT_EXCERPT_PATH, &current_excerpt_rows)?;
    if refresh_baseline {
        fs::create_dir_all("benchmarks")?;
        write_summary(BASELINE_PATH, &current_excerpt_rows)?;
    }

    metadata::write_metadata_json(METADATA_PATH)?;

    println!(
        "wrote {}, rows={}; wrote {}, rows={}; wrote {}, rows={}; wrote {}; baseline_refresh={}",
        SUMMARY_PATH,
        rows.len(),
        COMPARISON_PATH,
        comparisons.len(),
        CURRENT_EXCERPT_PATH,
        current_excerpt_rows.len(),
        VARIANCE_PATH,
        refresh_baseline
    );

    allocator::print_and_save_allocator_comparison(&rows)?;

    let mut regression_detected = false;
    for comp in &comparisons {
        let threshold = get_regression_threshold(&comp.benchmark);
        if comp.mean_ratio > threshold {
            eprintln!(
                "REGRESSION DETECTED: Benchmark '{}' mean ratio is {:.3} (exceeded threshold of {:.2})",
                comp.benchmark, comp.mean_ratio, threshold
            );
            regression_detected = true;
        }
    }

    if enforce_thresholds && !refresh_baseline && !missing_baseline_rows.is_empty() {
        return Err(io::Error::other(format!(
            "Missing selected benchmark rows for threshold enforcement: {}",
            missing_baseline_rows.join(", ")
        )));
    }

    if regression_detected && enforce_thresholds && !refresh_baseline {
        return Err(io::Error::other(
            "Performance regression detected. Gating threshold exceeded.",
        ));
    }

    Ok(())
}
