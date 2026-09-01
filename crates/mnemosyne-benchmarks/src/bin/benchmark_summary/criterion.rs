use super::report::SummaryRow;
use serde_json::Value;
use std::borrow::Cow;
use std::fs;
use std::io;
use std::path::Path;

/// Reads every benchmark estimate under one Criterion output root.
///
/// Row names are derived relative to `root`, so any root works — the run
/// directories that [`crate::repeat`] compares are not the default
/// `target/criterion`.
pub fn collect_estimates(root: &Path, rows: &mut Vec<SummaryRow<'static>>) -> io::Result<()> {
    collect_under(root, root, rows)
}

fn collect_under(root: &Path, path: &Path, rows: &mut Vec<SummaryRow<'static>>) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        if child.is_dir() {
            collect_under(root, &child, rows)?;
        } else if child.file_name().and_then(|name| name.to_str()) == Some("estimates.json")
            && child
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str())
                == Some("new")
            && let Some(row) = parse_estimates(root, &child)?
        {
            rows.push(row);
        }
    }

    Ok(())
}

fn parse_estimates(root: &Path, path: &Path) -> io::Result<Option<SummaryRow<'static>>> {
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
        benchmark: Cow::Owned(benchmark_name(root, path)),
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

/// Row name for one `estimates.json`, as its directory path relative to the
/// Criterion root that was scanned.
fn benchmark_name(root: &Path, path: &Path) -> String {
    let parent = path.parent().and_then(Path::parent).unwrap_or(path);
    let relative = parent.strip_prefix(root).unwrap_or(parent);
    normalize_path(relative)
}

fn normalize_path(path: &Path) -> String {
    let mut normalized = String::new();
    for component in path.components() {
        if !normalized.is_empty() {
            normalized.push('/');
        }
        normalized.push_str(&component.as_os_str().to_string_lossy());
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_path_joins_components_without_intermediate_vec() {
        let path = Path::new("allocator cycle latency")
            .join("mnemosyne")
            .join("large_8192");

        assert_eq!(
            normalize_path(&path),
            "allocator cycle latency/mnemosyne/large_8192"
        );
    }

    /// Row names are relative to whichever root was scanned, so the same
    /// benchmark reads identically from `target/criterion` and from the
    /// per-run roots the repeat check compares.
    #[test]
    fn benchmark_name_is_relative_to_the_scanned_root() {
        let estimates = Path::new("allocator cycle latency")
            .join("mnemosyne")
            .join("small_32")
            .join("new")
            .join("estimates.json");

        let default_root = Path::new("target/criterion");
        let run_root = Path::new("measurement/run2/target/criterion");

        assert_eq!(
            benchmark_name(default_root, &default_root.join(&estimates)),
            "allocator cycle latency/mnemosyne/small_32"
        );
        assert_eq!(
            benchmark_name(run_root, &run_root.join(&estimates)),
            "allocator cycle latency/mnemosyne/small_32"
        );
    }
}
