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
#[path = "benchmark_summary/repeat.rs"]
mod repeat;
#[path = "benchmark_summary/report.rs"]
mod report;
#[path = "benchmark_summary/threshold.rs"]
mod threshold;

use config::{
    BASELINE_PATH, COMPARISON_PATH, CRITERION_ROOT, CURRENT_EXCERPT_PATH, ENFORCE_THRESHOLDS_FLAG,
    METADATA_PATH, REFRESH_BASELINE_FLAG, REPEAT_SPREAD_FLAG, SUMMARY_PATH, VARIANCE_PATH,
    baseline_benchmarks,
};
use criterion::collect_estimates;
use report::{
    comparison_rows, missing_selected_benchmarks_message, read_summary, write_comparison,
    write_summary, write_summary_iter, write_variance_report,
};
use std::fs;
use std::io;
use std::path::Path;
use threshold::get_regression_threshold;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SummaryFlags {
    refresh_baseline: bool,
    enforce_thresholds: bool,
    /// Criterion output roots of repeated identical runs, collected from the
    /// positional arguments that follow [`REPEAT_SPREAD_FLAG`].
    repeat_spread_roots: Vec<String>,
}

fn parse_flags(args: impl IntoIterator<Item = String>) -> SummaryFlags {
    let mut flags = SummaryFlags::default();
    let mut collecting_roots = false;
    for arg in args {
        match arg.as_str() {
            REFRESH_BASELINE_FLAG => {
                flags.refresh_baseline = true;
                collecting_roots = false;
            }
            ENFORCE_THRESHOLDS_FLAG => {
                flags.enforce_thresholds = true;
                collecting_roots = false;
            }
            REPEAT_SPREAD_FLAG => collecting_roots = true,
            _ if collecting_roots => flags.repeat_spread_roots.push(arg),
            _ => {}
        }
    }
    flags
}

fn main() -> io::Result<()> {
    let flags = parse_flags(std::env::args().skip(1));
    // Repeat agreement reads several runs' roots and reports nothing about a
    // single run, so it is its own mode rather than an extra section here.
    if !flags.repeat_spread_roots.is_empty() {
        return repeat::check_repeat_agreement(&flags.repeat_spread_roots);
    }
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
    let comparison_count =
        write_comparison(COMPARISON_PATH, comparison_rows(&previous_baseline, &rows))?;

    let missing_baseline_rows = missing_selected_benchmarks_message(&rows);
    let current_excerpt_count = write_summary_iter(
        CURRENT_EXCERPT_PATH,
        baseline_benchmarks()
            .filter_map(|benchmark| rows.iter().find(|row| row.benchmark == benchmark)),
    )?;
    if flags.refresh_baseline {
        fs::create_dir_all("benchmarks")?;
        write_summary_iter(
            BASELINE_PATH,
            baseline_benchmarks()
                .filter_map(|benchmark| rows.iter().find(|row| row.benchmark == benchmark)),
        )?;
    }

    metadata::write_metadata_json(METADATA_PATH)?;

    println!(
        "wrote {}, rows={}; wrote {}, rows={}; wrote {}, rows={}; wrote {}; baseline_refresh={}",
        SUMMARY_PATH,
        rows.len(),
        COMPARISON_PATH,
        comparison_count,
        CURRENT_EXCERPT_PATH,
        current_excerpt_count,
        VARIANCE_PATH,
        flags.refresh_baseline
    );

    allocator::print_and_save_allocator_comparison(&rows)?;

    let mut regression_detected = false;
    for comp in comparison_rows(&previous_baseline, &rows) {
        let threshold = get_regression_threshold(comp.benchmark);
        if comp.mean_ratio > threshold {
            eprintln!(
                "REGRESSION DETECTED: Benchmark '{}' mean ratio is {:.3} (exceeded threshold of {:.2})",
                comp.benchmark, comp.mean_ratio, threshold
            );
            regression_detected = true;
        }
    }

    if flags.enforce_thresholds
        && !flags.refresh_baseline
        && let Some(missing_baseline_rows) = missing_baseline_rows
    {
        return Err(io::Error::other(format!(
            "Missing selected benchmark rows for threshold enforcement: {}",
            missing_baseline_rows
        )));
    }

    if regression_detected && flags.enforce_thresholds && !flags.refresh_baseline {
        return Err(io::Error::other(
            "Performance regression detected. Gating threshold exceeded.",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_summary_flags_without_order_dependency() {
        let flags = parse_flags([
            String::from("--ignored"),
            String::from(ENFORCE_THRESHOLDS_FLAG),
            String::from(REFRESH_BASELINE_FLAG),
        ]);

        assert_eq!(
            flags,
            SummaryFlags {
                refresh_baseline: true,
                enforce_thresholds: true,
                repeat_spread_roots: Vec::new(),
            }
        );
    }

    #[test]
    fn unknown_summary_flags_are_ignored() {
        let flags = parse_flags([String::from("--ignored")]);

        assert_eq!(flags, SummaryFlags::default());
    }

    #[test]
    fn repeat_spread_collects_the_roots_that_follow_it() {
        let flags = parse_flags([
            String::from(REPEAT_SPREAD_FLAG),
            String::from("run1/target/criterion"),
            String::from("run2/target/criterion"),
        ]);

        assert_eq!(
            flags.repeat_spread_roots,
            vec![
                String::from("run1/target/criterion"),
                String::from("run2/target/criterion"),
            ]
        );
    }

    /// A later flag ends root collection, so the two modes cannot silently
    /// swallow each other's arguments.
    #[test]
    fn a_following_flag_ends_root_collection() {
        let flags = parse_flags([
            String::from(REPEAT_SPREAD_FLAG),
            String::from("run1/target/criterion"),
            String::from(ENFORCE_THRESHOLDS_FLAG),
            String::from("stray"),
        ]);

        assert_eq!(
            flags.repeat_spread_roots,
            vec![String::from("run1/target/criterion")]
        );
        assert!(flags.enforce_thresholds);
    }
}
