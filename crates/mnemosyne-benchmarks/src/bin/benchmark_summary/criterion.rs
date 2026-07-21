use super::config::CRITERION_ROOT;
use super::report::SummaryRow;
use serde_json::Value;
use std::borrow::Cow;
use std::fs;
use std::io;
use std::path::Path;

pub fn collect_estimates(path: &Path, rows: &mut Vec<SummaryRow<'static>>) -> io::Result<()> {
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
            && let Some(row) = parse_estimates(&child)?
        {
            rows.push(row);
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
        benchmark: Cow::Owned(benchmark_name(path)),
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
}
