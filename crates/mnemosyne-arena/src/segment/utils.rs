//! Utility functions for segment alignment arithmetic.

/// Utility to align an address up to a given alignment boundary, returning `None` on overflow.
///
/// # Invariants
///
/// `align` must be a non-zero power of two.
#[inline(always)]
pub const fn checked_align_up(addr: usize, align: usize) -> Option<usize> {
    if align == 0 {
        return Some(addr);
    }
    let offset = align - 1;
    if let Some(sum) = addr.checked_add(offset) {
        Some(sum & !offset)
    } else {
        None
    }
}
