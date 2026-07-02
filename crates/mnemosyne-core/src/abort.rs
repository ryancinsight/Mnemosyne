//! Shared fatal-abort primitive for detected allocator corruption.

/// Terminates the process on detected heap corruption.
///
/// This is the single authoritative corruption sink: every corruption guard in
/// the allocator (double-free detection, out-of-bounds free-list links,
/// address-packing violations, thread-free cycle detection) routes here so the
/// termination behavior is defined once. When the `std` feature is active the
/// process aborts immediately with no unwinding; in a pure `no_std` build the
/// same condition panics with `msg` so the corruption reason survives in the
/// panic payload.
///
/// `msg` names the violated invariant. It is used as the panic message on the
/// `no_std` path; on the `std` path `std::process::abort()` carries no message,
/// matching the historical behavior of the per-site abort blocks.
#[inline(always)]
#[cold]
pub(crate) fn abort_on_corruption(msg: &str) -> ! {
    #[cfg(any(feature = "std", test))]
    {
        let _ = msg;
        std::process::abort();
    }
    #[cfg(not(any(feature = "std", test)))]
    {
        panic!("{msg}");
    }
}
