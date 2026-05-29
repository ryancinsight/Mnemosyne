//! Centralized allocation-request precondition checks.
//!
//! The allocator exposes three caller surfaces with subtly different
//! preconditions:
//!
//! - `thread_alloc(size, align)` accepts arbitrary unsafe inputs and must
//!   reject every invalid `(size, align)` combination before dispatch.
//! - `thread_alloc_layout(size, align)` is invoked from `GlobalAlloc::alloc`
//!   after `Layout::from_size_align` has already enforced the
//!   nonzero-power-of-two alignment contract, so it only needs to enforce
//!   Mnemosyne's allocator-specific upper bounds.
//! - `allocate_large_or_huge(size, align)` is reachable both from the
//!   high-alignment fast path inside `thread_alloc` and as the large-block
//!   fallback; it must apply the full validation surface because callers can
//!   bypass `thread_alloc`.
//!
//! Centralizing these checks here keeps every entry point in sync with a
//! single set of `const fn` predicates so a future change to
//! `MAX_ALLOC_SIZE`, the segment-alignment cap, or the power-of-two
//! requirement only edits one definition.

use crate::constants::{MAX_ALLOC_SIZE, SEGMENT_SIZE};

/// Returns `true` when `(size, align)` is a valid Mnemosyne allocation
/// request for the unsafe direct entry points.
///
/// The predicate is the conjunction of five clauses:
///
/// 1. `size != 0` — zero-size allocations are routed through the dedicated
///    null-return path because Mnemosyne does not return a unique sentinel
///    for `size == 0`.
/// 2. `size <= MAX_ALLOC_SIZE` — the payload bound preserves pointer-offset
///    arithmetic safety throughout the arena.
/// 3. `align != 0` — a zero alignment is not a valid `Layout` alignment.
/// 4. `align.is_power_of_two()` — Mnemosyne aligns through bitwise masks,
///    which assume a power-of-two alignment.
/// 5. `align <= SEGMENT_SIZE` — alignments above the segment alignment
///    would break the small-free classifier's segment-rounding header
///    recovery.
#[inline(always)]
pub const fn is_valid_alloc_request(size: usize, align: usize) -> bool {
    // `wrapping_sub`, not `size - 1`: for `size == 0` the latter underflows and
    // panics under debug overflow checks. Wrapping yields `usize::MAX`, which is
    // `>= MAX_ALLOC_SIZE`, so the branchless form still rejects zero (and any
    // `size > MAX_ALLOC_SIZE`) in both debug and release — equivalent to
    // `size != 0 && size <= MAX_ALLOC_SIZE` with no branch.
    size.wrapping_sub(1) < MAX_ALLOC_SIZE && align.is_power_of_two() && align <= SEGMENT_SIZE
}

/// Returns `true` when `(size, align)` is a valid Mnemosyne allocation
/// request for `Layout`-validated callers.
///
/// The caller is responsible for guaranteeing that `align` is a nonzero
/// power of two, which `Layout::from_size_align` already enforces. This
/// predicate therefore checks only the size bounds and the segment-alignment
/// upper limit, leaving the power-of-two contract to be `debug_assert!`ed
/// at the entry point.
#[inline(always)]
pub const fn is_valid_layout_alloc_request(size: usize, align: usize) -> bool {
    // `wrapping_sub` avoids the debug-build underflow panic on `size == 0`; see
    // `is_valid_alloc_request`. Branchless and equivalent to
    // `size != 0 && size <= MAX_ALLOC_SIZE`.
    size.wrapping_sub(1) < MAX_ALLOC_SIZE && align <= SEGMENT_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_validator_rejects_each_invalid_clause_independently() {
        // Valid baseline.
        assert!(is_valid_alloc_request(16, 8));
        // Zero size. Also guards against a `size - 1` formulation, which would
        // underflow and panic here under debug overflow checks; the validator
        // must use `wrapping_sub`.
        assert!(!is_valid_alloc_request(0, 8));
        // Above payload bound.
        assert!(!is_valid_alloc_request(MAX_ALLOC_SIZE + 1, 8));
        // Zero alignment.
        assert!(!is_valid_alloc_request(16, 0));
        // Non-power-of-two alignment.
        assert!(!is_valid_alloc_request(16, 6));
        // Alignment above segment alignment cap.
        assert!(!is_valid_alloc_request(16, SEGMENT_SIZE * 2));
        // Alignment at the segment alignment cap remains valid.
        assert!(is_valid_alloc_request(16, SEGMENT_SIZE));
    }

    #[test]
    fn layout_validator_trusts_power_of_two_invariant() {
        assert!(is_valid_layout_alloc_request(16, 8));
        // Zero size; also guards the debug-build underflow regression (must use
        // `wrapping_sub`, not `size - 1`).
        assert!(!is_valid_layout_alloc_request(0, 8));
        assert!(!is_valid_layout_alloc_request(MAX_ALLOC_SIZE + 1, 8));
        assert!(!is_valid_layout_alloc_request(16, SEGMENT_SIZE * 2));
        // The layout-validated variant does not reject zero alignment or
        // non-power-of-two values because Layout has already ruled them out.
        assert!(is_valid_layout_alloc_request(16, SEGMENT_SIZE));
    }
}
