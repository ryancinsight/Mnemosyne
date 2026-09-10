//! Benchmarking crate for the Mnemosyne memory allocator.

/// Whether the `snmalloc` comparator column is skipped for a given row.
///
/// On Windows `x86_64`, `snmalloc`'s huge-allocation path is unreliable under
/// the MinGW linker shims, so the huge rows omit the `snmalloc` column. The
/// huge row is named `"huge/2m"` in the latency/cross-thread/usable-size groups
/// and `"huge_shrink_4m_to_2m"` in the realloc group; matching both keeps this
/// predicate a single source of truth while remaining behaviourally identical
/// to the per-group checks it replaces (each group only ever passes its own
/// huge row name). On every other platform no row is skipped.
#[cfg(feature = "snmalloc")]
#[inline]
pub fn snmalloc_skips(name: &str) -> bool {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        name == "huge/2m" || name == "huge_shrink_4m_to_2m"
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = name;
        false
    }
}

#[cfg(all(test, feature = "snmalloc"))]
mod tests {
    use super::snmalloc_skips;

    #[test]
    fn snmalloc_skip_gate_only_applies_to_windows_huge_rows() {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            assert!(snmalloc_skips("huge/2m"));
            assert!(snmalloc_skips("huge_shrink_4m_to_2m"));
            assert!(!snmalloc_skips("small_32"));
            assert!(!snmalloc_skips("large_8192"));
        }

        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            assert!(!snmalloc_skips("huge/2m"));
            assert!(!snmalloc_skips("huge_shrink_4m_to_2m"));
            assert!(!snmalloc_skips("small_32"));
            assert!(!snmalloc_skips("large_8192"));
        }
    }
}
