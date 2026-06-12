use super::report::SummaryRow;
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io;

pub fn print_and_save_allocator_comparison(rows: &[SummaryRow]) -> io::Result<()> {
    let mut table: BTreeMap<(&str, &str), AllocatorComparison> = BTreeMap::new();

    for row in rows {
        let Some((group, allocator, sub_bench)) = split_allocator_benchmark(&row.benchmark) else {
            continue;
        };

        let Some(kind) = classify_allocator(allocator) else {
            continue;
        };

        let entry = table.entry((group, sub_bench)).or_default();
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
        let name = BenchmarkName { group, sub_bench };
        let mnemosyne = NumericCell(comparison.mnemosyne);
        let system = NumericCell(comparison.system);
        let mimalloc = NumericCell(comparison.mimalloc);
        let rpmalloc = NumericCell(comparison.rpmalloc);
        let snmalloc = NumericCell(comparison.snmalloc);
        let jemalloc = NumericCell(comparison.jemalloc);

        println!(
            "{:<45} {:<15} {:<15} {:<15} {:<15} {:<15} {:<15}",
            name, mnemosyne, system, mimalloc, rpmalloc, snmalloc, jemalloc
        );

        writeln!(
            markdown,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            name,
            mnemosyne,
            system,
            mimalloc,
            rpmalloc,
            snmalloc,
            jemalloc,
            RatioCell(comparison.mnemosyne, comparison.system),
            RatioCell(comparison.mnemosyne, comparison.mimalloc),
            RatioCell(comparison.mnemosyne, comparison.rpmalloc),
            RatioCell(comparison.mnemosyne, comparison.snmalloc),
            RatioCell(comparison.mnemosyne, comparison.jemalloc)
        )
        .expect("writing allocator comparison markdown into String cannot fail");
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

#[derive(Clone, Copy)]
struct BenchmarkName<'a> {
    group: &'a str,
    sub_bench: &'a str,
}

impl fmt::Display for BenchmarkName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.sub_bench.is_empty() {
            f.write_str(self.group)
        } else {
            write!(f, "{}/{}", self.group, self.sub_bench)
        }
    }
}

#[derive(Clone, Copy)]
struct NumericCell(Option<f64>);

impl fmt::Display for NumericCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(value) => write!(f, "{value:.3}"),
            None => f.write_str("N/A"),
        }
    }
}

#[derive(Clone, Copy)]
struct RatioCell(Option<f64>, Option<f64>);

impl fmt::Display for RatioCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.0, self.1) {
            (Some(mnemosyne), Some(other)) => write!(f, "{:.2}x", mnemosyne / other),
            _ => f.write_str("N/A"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
