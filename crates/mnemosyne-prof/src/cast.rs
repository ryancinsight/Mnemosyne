//! The crate's one cast module: conversions the standard library has no
//! non-`as` form for.

#![expect(
    clippy::as_conversions,
    reason = "std has no non-`as` form converting a function pointer to a data pointer"
)]

use core::ffi::c_void;

/// Publishes a trace hook as the opaque pointer stored in the hook slots.
///
/// The slots are `AtomicPtr<c_void>`, so a function pointer enters them as a
/// data pointer. `hooks` reconstructs the exact `unsafe extern "C"` signature
/// from a slot value, which is sound because every non-null slot value comes
/// from this function: `register_alloc_hook` and `register_free_hook` store
/// its result or null, `reset_profiler_for_testing` stores null, and `hooks`
/// transmutes only after checking the loaded value for null.
#[inline]
pub(crate) fn hook_slot_pointer(hook: unsafe extern "C" fn(*mut c_void, usize)) -> *mut c_void {
    hook as *mut c_void
}
