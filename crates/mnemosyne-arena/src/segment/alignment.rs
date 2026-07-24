//! Segment alignment arithmetic.

/// Align an address up to a given alignment boundary, returning `None` on overflow.
///
/// # Invariants
///
/// `align` must be a non-zero power of two.
#[inline(always)]
pub const fn checked_align_up(addr: usize, align: usize) -> Option<usize> {
    debug_assert!(
        align == 0 || align.is_power_of_two(),
        "align must be a power of two"
    );
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
