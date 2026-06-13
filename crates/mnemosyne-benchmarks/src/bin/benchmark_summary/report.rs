use super::config::BASELINE_BENCHMARKS;
use super::csv::{escape_csv, parse_summary_line};
use super::threshold::variance_threshold;
use std::borrow::Cow;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

#[derive(Clone)]
pub struct SummaryRow<'a> {
    pub benchmark: Cow<'a, str>,
    pub mean_ns: f64,
    pub median_ns: f64,
    pub mean_ci_lower_ns: Option<f64>,
    pub mean_ci_upper_ns: Option<f64>,
}

pub struct ComparisonRow<'a> {
    pub benchmark: &'a str,
    pub current_mean_ns: f64,
    pub baseline_mean_ns: f64,
    pub mean_ratio: f64,
    pub current_median_ns: f64,
    pub baseline_median_ns: f64,
    pub median_ratio: f64,
}

pub fn read_summary(contents: &str) -> io::Result<Vec<SummaryRow<'_>>> {
    let mut rows = Vec::new();
    for (line_index, line) in contents.lines().enumerate() {
        if line_index == 0 || line.trim().is_empty() {
            continue;
        }

        let fields = parse_summary_line(line).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid benchmark summary CSV line {line_index}"),
            )
        })?;
        rows.push(SummaryRow {
            benchmark: fields.0,
            mean_ns: fields.1,
            median_ns: fields.2,
            mean_ci_lower_ns: None,
            mean_ci_upper_ns: None,
        });
    }

    Ok(rows)
}

pub fn missing_selected_benchmarks_message(rows: &[SummaryRow]) -> Option<String> {
    let mut missing = BASELINE_BENCHMARKS
        .iter()
        .copied()
        .filter(|benchmark| !rows.iter().any(|row| row.benchmark == *benchmark));
    let first = missing.next()?;
    let mut message = String::from(first);
    for benchmark in missing {
        message.push_str(", ");
        message.push_str(benchmark);
    }
    Some(message)
}

pub fn write_summary(path: &str, rows: &[SummaryRow]) -> io::Result<()> {
    write_summary_iter(path, rows.iter()).map(|_| ())
}

pub fn write_summary_iter<'row, 'benchmark, I>(path: &str, rows: I) -> io::Result<usize>
where
    'benchmark: 'row,
    I: IntoIterator<Item = &'row SummaryRow<'benchmark>>,
{
    ensure_parent_dir(path)?;
    let mut output = File::create(path)?;
    writeln!(
        output,
        "benchmark,mean_point_estimate_ns,median_point_estimate_ns"
    )?;
    let mut count = 0;
    for row in rows {
        writeln!(
            output,
            "{},{:.6},{:.6}",
            escape_csv(&row.benchmark),
            row.mean_ns,
            row.median_ns
        )?;
        count += 1;
    }
    Ok(count)
}

pub fn write_variance_report(path: &str, rows: &[SummaryRow]) -> io::Result<()> {
    ensure_parent_dir(path)?;
    let mut output = File::create(path)?;
    writeln!(
        output,
        "benchmark,mean_point_estimate_ns,mean_ci_lower_ns,mean_ci_upper_ns,mean_ci_relative_width,unstable"
    )?;
    for row in rows {
        let (lower, upper, relative_width, unstable) =
            match (row.mean_ci_lower_ns, row.mean_ci_upper_ns) {
                (Some(lower), Some(upper)) if row.mean_ns > 0.0 => {
                    let relative_width = (upper - lower) / row.mean_ns;
                    (
                        format!("{lower:.6}"),
                        format!("{upper:.6}"),
                        format!("{relative_width:.6}"),
                        relative_width > variance_threshold(&row.benchmark),
                    )
                }
                _ => (
                    "N/A".to_string(),
                    "N/A".to_string(),
                    "N/A".to_string(),
                    false,
                ),
            };
        writeln!(
            output,
            "{},{:.6},{},{},{},{}",
            escape_csv(&row.benchmark),
            row.mean_ns,
            lower,
            upper,
            relative_width,
            unstable
        )?;
    }
    Ok(())
}

pub fn comparison_rows<'row>(
    baseline: &'row [SummaryRow<'_>],
    current: &'row [SummaryRow<'_>],
) -> impl Iterator<Item = ComparisonRow<'row>> + 'row {
    baseline.iter().filter_map(|baseline_row| {
        let current_row = current
            .iter()
            .find(|row| row.benchmark == baseline_row.benchmark)?;
        Some(ComparisonRow {
            benchmark: baseline_row.benchmark.as_ref(),
            current_mean_ns: current_row.mean_ns,
            baseline_mean_ns: baseline_row.mean_ns,
            mean_ratio: current_row.mean_ns / baseline_row.mean_ns,
            current_median_ns: current_row.median_ns,
            baseline_median_ns: baseline_row.median_ns,
            median_ratio: current_row.median_ns / baseline_row.median_ns,
        })
    })
}

pub fn write_comparison<'row, I>(path: &str, rows: I) -> io::Result<usize>
where
    I: IntoIterator<Item = ComparisonRow<'row>>,
{
    ensure_parent_dir(path)?;
    let mut output = File::create(path)?;
    writeln!(
        output,
        "benchmark,current_mean_ns,baseline_mean_ns,mean_ratio,current_median_ns,baseline_median_ns,median_ratio"
    )?;
    let mut count = 0;
    for row in rows {
        writeln!(
            output,
            "{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6}",
            escape_csv(row.benchmark),
            row.current_mean_ns,
            row.baseline_mean_ns,
            row.mean_ratio,
            row.current_median_ns,
            row.baseline_median_ns,
            row.median_ratio
        )?;
        count += 1;
    }
    Ok(count)
}

fn ensure_parent_dir(path: &str) -> io::Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_current_to_baseline_ratios() {
        let baseline = [SummaryRow {
            benchmark: Cow::Borrowed("allocator cycle latency/mnemosyne/small_32"),
            mean_ns: 10.0,
            median_ns: 20.0,
            mean_ci_lower_ns: None,
            mean_ci_upper_ns: None,
        }];
        let current = [SummaryRow {
            benchmark: Cow::Borrowed("allocator cycle latency/mnemosyne/small_32"),
            mean_ns: 15.0,
            median_ns: 10.0,
            mean_ci_lower_ns: None,
            mean_ci_upper_ns: None,
        }];

        let mut comparison = comparison_rows(&baseline, &current);
        let first = comparison
            .next()
            .expect("matching current row must produce one comparison");

        assert_eq!(first.mean_ratio, 1.5);
        assert_eq!(first.median_ratio, 0.5);
        assert!(
            comparison.next().is_none(),
            "one baseline row must produce only one comparison"
        );
    }

    #[test]
    fn reports_missing_selected_baseline_rows() {
        let current = [SummaryRow {
            benchmark: Cow::Borrowed("allocator cycle latency/mnemosyne/small_32"),
            mean_ns: 10.0,
            median_ns: 10.0,
            mean_ci_lower_ns: None,
            mean_ci_upper_ns: None,
        }];

        let missing = missing_selected_benchmarks_message(&current)
            .expect("incomplete current rows must report missing selected benchmarks");

        assert!(
            missing.contains("threaded saturated small allocation cycles/mnemosyne"),
            "missing selected rows must include absent threshold-gated threaded benchmark"
        );
        assert!(
            !missing.contains("allocator cycle latency/mnemosyne/small_32"),
            "present selected rows must not be reported missing"
        );
    }

    #[test]
    fn summary_writer_creates_missing_parent_directories() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock must be after Unix epoch for temp path generation")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mnemosyne-benchmark-summary-{}-{nonce}",
            std::process::id(),
        ));
        let output = root.join("nested").join("summary.csv");
        let row = SummaryRow {
            benchmark: Cow::Borrowed("allocator cycle latency/mnemosyne/small_32"),
            mean_ns: 1.25,
            median_ns: 1.0,
            mean_ci_lower_ns: None,
            mean_ci_upper_ns: None,
        };

        write_summary(
            output
                .to_str()
                .expect("temporary benchmark-summary path must be valid UTF-8"),
            &[row],
        )
        .expect("summary writer must create missing parent directories");

        let contents = std::fs::read_to_string(&output).expect("summary output must be readable");
        assert!(
            contents.contains("allocator cycle latency/mnemosyne/small_32,1.250000,1.000000"),
            "summary output must contain the written benchmark row, got {contents:?}"
        );

        std::fs::remove_dir_all(&root)
            .expect("temporary benchmark-summary directory cleanup failed");
    }

    #[test]
    fn summary_iter_writer_reports_written_row_count() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock must be after Unix epoch for temp path generation")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mnemosyne-benchmark-summary-iter-{}-{nonce}",
            std::process::id(),
        ));
        let output = root.join("summary.csv");
        let rows = [
            SummaryRow {
                benchmark: Cow::Borrowed("allocator cycle latency/mnemosyne/small_32"),
                mean_ns: 1.25,
                median_ns: 1.0,
                mean_ci_lower_ns: None,
                mean_ci_upper_ns: None,
            },
            SummaryRow {
                benchmark: Cow::Borrowed("allocator cycle latency/mnemosyne/medium_1024"),
                mean_ns: 2.5,
                median_ns: 2.0,
                mean_ci_lower_ns: None,
                mean_ci_upper_ns: None,
            },
        ];

        let count = write_summary_iter(
            output
                .to_str()
                .expect("temporary benchmark-summary path must be valid UTF-8"),
            rows.iter(),
        )
        .expect("summary iterator writer must create output");

        assert_eq!(count, 2);
        let contents = std::fs::read_to_string(&output).expect("summary output must be readable");
        assert!(
            contents.contains("allocator cycle latency/mnemosyne/medium_1024,2.500000,2.000000"),
            "summary output must contain the second streamed row, got {contents:?}"
        );

        std::fs::remove_dir_all(&root)
            .expect("temporary benchmark-summary directory cleanup failed");
    }
}
