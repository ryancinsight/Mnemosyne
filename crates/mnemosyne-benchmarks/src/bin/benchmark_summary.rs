use serde_json::Value;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

const CRITERION_ROOT: &str = "target/criterion";
const SUMMARY_PATH: &str = "target/criterion/benchmark_summary.csv";
const COMPARISON_PATH: &str = "target/criterion/benchmark_baseline_comparison.csv";
const CURRENT_EXCERPT_PATH: &str = "target/criterion/allocator_current_excerpt.csv";
const VARIANCE_PATH: &str = "target/criterion/benchmark_variance.csv";
const METADATA_PATH: &str = "target/criterion/benchmark_metadata.json";
const BASELINE_PATH: &str = "benchmarks/allocator_baseline_excerpt.csv";
const REFRESH_BASELINE_FLAG: &str = "--refresh-baseline";
const ENFORCE_THRESHOLDS_FLAG: &str = "--enforce-thresholds";
const ACTIVE_GROUPS: [&str; 12] = [
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
const BASELINE_BENCHMARKS: [&str; 7] = [
    "allocator cycle latency/mnemosyne/small_32",
    "allocator cycle latency/mnemosyne/medium_1024",
    "allocator cycle latency/mnemosyne/large_8192",
    "allocator burst retention/mnemosyne/small_32",
    "cross-thread free handoff/mnemosyne/small_32",
    "threaded saturated small allocation cycles/mnemosyne",
    "segment cache eviction/mnemosyne",
];

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
    rows.retain(|row| is_active_benchmark(&row.benchmark));
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

    write_metadata_json(METADATA_PATH)?;

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

    // Print side-by-side allocator comparisons and save allocator_comparison.md
    print_and_save_allocator_comparison(&rows)?;

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

fn get_regression_threshold(benchmark: &str) -> f64 {
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

fn is_active_benchmark(benchmark: &str) -> bool {
    ACTIVE_GROUPS
        .iter()
        .any(|group| benchmark.starts_with(group))
}

fn missing_selected_benchmarks(rows: &[SummaryRow]) -> Vec<&'static str> {
    BASELINE_BENCHMARKS
        .iter()
        .copied()
        .filter(|benchmark| !rows.iter().any(|row| row.benchmark == *benchmark))
        .collect()
}

fn print_and_save_allocator_comparison(rows: &[SummaryRow]) -> io::Result<()> {
    use std::collections::BTreeMap;

    let mut table: BTreeMap<(String, String), AllocatorComparison> = BTreeMap::new();

    for row in rows {
        let Some((group, allocator, sub_bench)) = split_allocator_benchmark(&row.benchmark) else {
            continue;
        };

        let Some(kind) = classify_allocator(allocator) else {
            continue;
        };

        let entry = table
            .entry((group.to_owned(), sub_bench.to_owned()))
            .or_default();
        match kind {
            AllocatorKind::Mnemosyne => entry.mnemosyne = Some(row.mean_ns),
            AllocatorKind::System => entry.system = Some(row.mean_ns),
            AllocatorKind::MiMalloc => entry.mimalloc = Some(row.mean_ns),
            AllocatorKind::RpMalloc => entry.rpmalloc = Some(row.mean_ns),
            AllocatorKind::SnMalloc => entry.snmalloc = Some(row.mean_ns),
            AllocatorKind::Jemalloc => entry.jemalloc = Some(row.mean_ns),
        }
    }

    let mut markdown = String::new();
    markdown.push_str("# Allocator Performance Comparison\n\n");
    markdown.push_str("| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | RpMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs RpMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |\n");
    markdown.push_str(
        "| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |\n",
    );

    println!("\nAllocator Comparisons (Current Run):");
    println!(
        "============================================================================================================================================"
    );
    println!(
        "{:<45} {:<15} {:<15} {:<15} {:<15} {:<15} {:<15}",
        "Benchmark",
        "Mnemosyne (ns)",
        "System (ns)",
        "MiMalloc (ns)",
        "RpMalloc (ns)",
        "SnMalloc (ns)",
        "Jemalloc (ns)"
    );
    println!(
        "--------------------------------------------------------------------------------------------------------------------------------------------"
    );

    for ((group, sub_bench), comparison) in &table {
        let name = if sub_bench.is_empty() {
            group.clone()
        } else {
            format!("{}/{}", group, sub_bench)
        };

        let mne_str = comparison
            .mnemosyne
            .map_or("N/A".to_string(), |v| format!("{:.3}", v));
        let sys_str = comparison
            .system
            .map_or("N/A".to_string(), |v| format!("{:.3}", v));
        let mi_str = comparison
            .mimalloc
            .map_or("N/A".to_string(), |v| format!("{:.3}", v));
        let rpm_str = comparison
            .rpmalloc
            .map_or("N/A".to_string(), |v| format!("{:.3}", v));
        let sn_str = comparison
            .snmalloc
            .map_or("N/A".to_string(), |v| format!("{:.3}", v));
        let je_str = comparison
            .jemalloc
            .map_or("N/A".to_string(), |v| format!("{:.3}", v));

        println!(
            "{:<45} {:<15} {:<15} {:<15} {:<15} {:<15} {:<15}",
            name, mne_str, sys_str, mi_str, rpm_str, sn_str, je_str
        );

        let vs_sys = match (comparison.mnemosyne, comparison.system) {
            (Some(mn_v), Some(sys_v)) => format!("{:.2}x", mn_v / sys_v),
            _ => "N/A".to_string(),
        };
        let vs_mi = match (comparison.mnemosyne, comparison.mimalloc) {
            (Some(mn_v), Some(mi_v)) => format!("{:.2}x", mn_v / mi_v),
            _ => "N/A".to_string(),
        };
        let vs_rpm = match (comparison.mnemosyne, comparison.rpmalloc) {
            (Some(mn_v), Some(rpm_v)) => format!("{:.2}x", mn_v / rpm_v),
            _ => "N/A".to_string(),
        };
        let vs_sn = match (comparison.mnemosyne, comparison.snmalloc) {
            (Some(mn_v), Some(sn_v)) => format!("{:.2}x", mn_v / sn_v),
            _ => "N/A".to_string(),
        };
        let vs_je = match (comparison.mnemosyne, comparison.jemalloc) {
            (Some(mn_v), Some(je_v)) => format!("{:.2}x", mn_v / je_v),
            _ => "N/A".to_string(),
        };
        markdown.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            name,
            mne_str,
            sys_str,
            mi_str,
            rpm_str,
            sn_str,
            je_str,
            vs_sys,
            vs_mi,
            vs_rpm,
            vs_sn,
            vs_je
        ));
    }
    println!(
        "============================================================================================================================================\n"
    );

    fs::create_dir_all("benchmarks")?;
    fs::write("benchmarks/allocator_comparison.md", markdown)?;

    Ok(())
}

fn split_allocator_benchmark(benchmark: &str) -> Option<(&str, &str, &str)> {
    let (group, tail) = benchmark.split_once('/')?;
    match tail.split_once('/') {
        Some((allocator, sub_bench)) => Some((group, allocator, sub_bench)),
        None => Some((group, tail, "")),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AllocatorKind {
    Mnemosyne,
    System,
    MiMalloc,
    RpMalloc,
    SnMalloc,
    Jemalloc,
}

fn classify_allocator(allocator: &str) -> Option<AllocatorKind> {
    if allocator.eq_ignore_ascii_case("mnemosyne") {
        Some(AllocatorKind::Mnemosyne)
    } else if allocator.eq_ignore_ascii_case("system") {
        Some(AllocatorKind::System)
    } else if allocator.eq_ignore_ascii_case("mimalloc") {
        Some(AllocatorKind::MiMalloc)
    } else if allocator.eq_ignore_ascii_case("rpmalloc") {
        Some(AllocatorKind::RpMalloc)
    } else if allocator.eq_ignore_ascii_case("snmalloc") {
        Some(AllocatorKind::SnMalloc)
    } else if allocator.eq_ignore_ascii_case("jemalloc") {
        Some(AllocatorKind::Jemalloc)
    } else {
        None
    }
}

#[derive(Default)]
struct AllocatorComparison {
    mnemosyne: Option<f64>,
    system: Option<f64>,
    mimalloc: Option<f64>,
    rpmalloc: Option<f64>,
    snmalloc: Option<f64>,
    jemalloc: Option<f64>,
}

fn write_metadata_json(path: &str) -> io::Result<()> {
    let rustc_version = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let os_family = if cfg!(target_family = "windows") {
        "windows"
    } else if cfg!(target_family = "unix") {
        "unix"
    } else {
        "unknown"
    };

    let target_arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "unknown"
    };

    let timestamp_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let metadata = serde_json::json!({
        "rustc_version": rustc_version,
        "os_family": os_family,
        "target_arch": target_arch,
        "timestamp_secs": timestamp_secs,
    });

    let mut file = File::create(path)?;
    serde_json::to_writer_pretty(&mut file, &metadata).map_err(io::Error::other)?;
    Ok(())
}

#[derive(Clone)]
struct SummaryRow<'a> {
    benchmark: std::borrow::Cow<'a, str>,
    mean_ns: f64,
    median_ns: f64,
    mean_ci_lower_ns: Option<f64>,
    mean_ci_upper_ns: Option<f64>,
}

struct ComparisonRow<'a> {
    benchmark: std::borrow::Cow<'a, str>,
    current_mean_ns: f64,
    baseline_mean_ns: f64,
    mean_ratio: f64,
    current_median_ns: f64,
    baseline_median_ns: f64,
    median_ratio: f64,
}

fn read_summary<'a>(contents: &'a str) -> io::Result<Vec<SummaryRow<'a>>> {
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

fn parse_summary_line<'a>(line: &'a str) -> Option<(std::borrow::Cow<'a, str>, f64, f64)> {
    let fields = parse_csv_line_cow(line);
    if fields.len() != 3 {
        return None;
    }

    let mean_ns = fields[1].parse().ok()?;
    let median_ns = fields[2].parse().ok()?;
    Some((fields[0].clone(), mean_ns, median_ns))
}

fn parse_csv_line_cow<'a>(line: &'a str) -> Vec<std::borrow::Cow<'a, str>> {
    let mut fields = Vec::new();
    let mut chars = line.char_indices().peekable();
    let mut in_quotes = false;
    let mut start = 0;
    let mut has_escapes = false;

    while let Some((idx, ch)) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek().map(|&(_, c)| c) == Some('"') => {
                has_escapes = true;
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                let segment = &line[start..idx];
                fields.push(process_segment(segment, has_escapes));
                start = idx + 1;
                has_escapes = false;
            }
            _ => {}
        }
    }
    let segment = &line[start..];
    fields.push(process_segment(segment, has_escapes));
    fields
}

fn process_segment<'a>(segment: &'a str, has_escapes: bool) -> std::borrow::Cow<'a, str> {
    let trimmed = segment.trim();
    let stripped = if trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    if has_escapes {
        std::borrow::Cow::Owned(stripped.replace("\"\"", "\""))
    } else {
        std::borrow::Cow::Borrowed(stripped)
    }
}

fn write_summary(path: &str, rows: &[SummaryRow]) -> io::Result<()> {
    let mut output = File::create(path)?;
    writeln!(
        output,
        "benchmark,mean_point_estimate_ns,median_point_estimate_ns"
    )?;
    for row in rows {
        writeln!(
            output,
            "{},{:.6},{:.6}",
            escape_csv(&row.benchmark),
            row.mean_ns,
            row.median_ns
        )?;
    }
    Ok(())
}

fn write_variance_report(path: &str, rows: &[SummaryRow]) -> io::Result<()> {
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

fn variance_threshold(benchmark: &str) -> f64 {
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

fn compare_to_baseline<'a, 'b>(
    baseline: &[SummaryRow<'a>],
    current: &[SummaryRow<'b>],
) -> Vec<ComparisonRow<'a>> {
    baseline
        .iter()
        .filter_map(|baseline_row| {
            let current_row = current
                .iter()
                .find(|row| row.benchmark == baseline_row.benchmark)?;
            Some(ComparisonRow {
                benchmark: baseline_row.benchmark.clone(),
                current_mean_ns: current_row.mean_ns,
                baseline_mean_ns: baseline_row.mean_ns,
                mean_ratio: current_row.mean_ns / baseline_row.mean_ns,
                current_median_ns: current_row.median_ns,
                baseline_median_ns: baseline_row.median_ns,
                median_ratio: current_row.median_ns / baseline_row.median_ns,
            })
        })
        .collect()
}

fn write_comparison(path: &str, rows: &[ComparisonRow]) -> io::Result<()> {
    let mut output = File::create(path)?;
    writeln!(
        output,
        "benchmark,current_mean_ns,baseline_mean_ns,mean_ratio,current_median_ns,baseline_median_ns,median_ratio"
    )?;
    for row in rows {
        writeln!(
            output,
            "{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6}",
            escape_csv(&row.benchmark),
            row.current_mean_ns,
            row.baseline_mean_ns,
            row.mean_ratio,
            row.current_median_ns,
            row.baseline_median_ns,
            row.median_ratio
        )?;
    }
    Ok(())
}

fn collect_estimates(path: &Path, rows: &mut Vec<SummaryRow<'static>>) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        if child.is_dir() {
            collect_estimates(&child, rows)?;
        } else if child.file_name().and_then(|name| name.to_str()) == Some("estimates.json")
            && child
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str())
                == Some("new")
        {
            if let Some(row) = parse_estimates(&child)? {
                rows.push(row);
            }
        }
    }

    Ok(())
}

fn parse_estimates(path: &Path) -> io::Result<Option<SummaryRow<'static>>> {
    let contents = fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&contents).map_err(io::Error::other)?;
    let mean_ns = match estimate_point(&value, "mean") {
        Some(point) => point,
        None => return Ok(None),
    };
    let mean_ci_lower_ns = estimate_ci_bound(&value, "mean", "lower_bound");
    let mean_ci_upper_ns = estimate_ci_bound(&value, "mean", "upper_bound");
    let median_ns = match estimate_point(&value, "median") {
        Some(point) => point,
        None => return Ok(None),
    };

    Ok(Some(SummaryRow {
        benchmark: std::borrow::Cow::Owned(benchmark_name(path)),
        mean_ns,
        median_ns,
        mean_ci_lower_ns,
        mean_ci_upper_ns,
    }))
}

fn estimate_point(value: &Value, name: &str) -> Option<f64> {
    value.get(name)?.get("point_estimate")?.as_f64()
}

fn estimate_ci_bound(value: &Value, estimate: &str, bound: &str) -> Option<f64> {
    value
        .get(estimate)?
        .get("confidence_interval")?
        .get(bound)?
        .as_f64()
}

fn benchmark_name(path: &Path) -> String {
    let parent = path.parent().and_then(Path::parent).unwrap_or(path);
    let relative = parent
        .strip_prefix(Path::new(CRITERION_ROOT))
        .unwrap_or(parent);
    normalize_path(relative)
}

fn normalize_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn escape_csv(value: &str) -> String {
    if value.contains(',') || value.contains('"') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_escaped_summary_row() {
        let row = parse_summary_line("\"allocator, \"\"quoted\"\"\",1.250000,2.500000")
            .expect("valid escaped row");

        assert_eq!(row.0, "allocator, \"quoted\"");
        assert_eq!(row.1, 1.25);
        assert_eq!(row.2, 2.5);
    }

    #[test]
    fn computes_current_to_baseline_ratios() {
        let baseline = [SummaryRow {
            benchmark: std::borrow::Cow::Borrowed("allocator cycle latency/mnemosyne/small_32"),
            mean_ns: 10.0,
            median_ns: 20.0,
            mean_ci_lower_ns: None,
            mean_ci_upper_ns: None,
        }];
        let current = [SummaryRow {
            benchmark: std::borrow::Cow::Borrowed("allocator cycle latency/mnemosyne/small_32"),
            mean_ns: 15.0,
            median_ns: 10.0,
            mean_ci_lower_ns: None,
            mean_ci_upper_ns: None,
        }];

        let comparison = compare_to_baseline(&baseline, &current);

        assert_eq!(comparison.len(), 1);
        assert_eq!(comparison[0].mean_ratio, 1.5);
        assert_eq!(comparison[0].median_ratio, 0.5);
    }

    #[test]
    fn reports_missing_selected_baseline_rows() {
        let current = [SummaryRow {
            benchmark: std::borrow::Cow::Borrowed("allocator cycle latency/mnemosyne/small_32"),
            mean_ns: 10.0,
            median_ns: 10.0,
            mean_ci_lower_ns: None,
            mean_ci_upper_ns: None,
        }];

        let missing = missing_selected_benchmarks(&current);

        assert!(
            missing.contains(&"threaded saturated small allocation cycles/mnemosyne"),
            "missing selected rows must include absent threshold-gated threaded benchmark"
        );
        assert!(
            !missing.contains(&"allocator cycle latency/mnemosyne/small_32"),
            "present selected rows must not be reported missing"
        );
    }

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

    #[test]
    fn allocator_benchmark_split_handles_optional_sub_benchmark_without_vec() {
        assert_eq!(
            split_allocator_benchmark("allocator cycle latency/mnemosyne/small_32"),
            Some(("allocator cycle latency", "mnemosyne", "small_32"))
        );
        assert_eq!(
            split_allocator_benchmark("segment cache eviction/mnemosyne"),
            Some(("segment cache eviction", "mnemosyne", ""))
        );
        assert_eq!(split_allocator_benchmark("malformed"), None);
    }

    #[test]
    fn allocator_classification_is_exact_not_substring_based() {
        assert_eq!(
            classify_allocator("mnemosyne"),
            Some(AllocatorKind::Mnemosyne)
        );
        assert_eq!(
            classify_allocator("Mnemosyne"),
            Some(AllocatorKind::Mnemosyne)
        );
        assert_eq!(classify_allocator("system"), Some(AllocatorKind::System));
        assert_eq!(
            classify_allocator("rpmalloc"),
            Some(AllocatorKind::RpMalloc)
        );
        assert_eq!(classify_allocator("notmnemosyne"), None);
        assert_eq!(classify_allocator("notrpmalloc"), None);
    }
}
