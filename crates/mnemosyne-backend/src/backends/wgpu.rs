//! `WgpuStagingBackend` — adapter that hooks wgpu's allocator callbacks
//! into the Mnemosyne `MemoryBackend` seam.
//!
//! Consumers construct one static [`WgpuCallbacks`] pair and publish it through
//! [`register_wgpu_callbacks`]. One atomic pointer makes the pair immutable for
//! the process lifetime, so allocation and deallocation cannot observe
//! different callback generations. The other trait methods use the trait
//! defaults (resets/guards/decommit return `false`, no SUPPORTS_* flags).

use core::fmt;
use core::sync::atomic::{AtomicPtr, Ordering};
use mnemosyne_core::MemoryBackend;

/// Allocation callback installed for [`WgpuStagingBackend`].
pub type WgpuAllocateCallback = unsafe extern "C" fn(usize) -> *mut u8;

/// Deallocation callback installed for [`WgpuStagingBackend`].
pub type WgpuDeallocateCallback = unsafe extern "C" fn(*mut u8, usize) -> bool;

/// Immutable allocation/deallocation contract for [`WgpuStagingBackend`].
#[derive(Clone, Copy)]
pub struct WgpuCallbacks {
    allocate: WgpuAllocateCallback,
    deallocate: WgpuDeallocateCallback,
}

impl WgpuCallbacks {
    /// Constructs one process-lifetime WGPU callback pair.
    ///
    /// # Safety
    ///
    /// `allocate` must return either null or a pointer to at least `size`
    /// writable bytes whose lifetime remains valid until `deallocate` receives
    /// the same pointer and size. `deallocate` must release only pointers from
    /// this `allocate` function. Neither callback may unwind across its FFI
    /// boundary.
    #[must_use]
    pub const unsafe fn new(
        allocate: WgpuAllocateCallback,
        deallocate: WgpuDeallocateCallback,
    ) -> Self {
        Self {
            allocate,
            deallocate,
        }
    }
}

/// A different WGPU callback pair already owns the process-global registry.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WgpuCallbackRegistrationError;

impl fmt::Display for WgpuCallbackRegistrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a different WGPU callback pair is already registered")
    }
}

impl core::error::Error for WgpuCallbackRegistrationError {}

struct WgpuCallbackRegistry {
    callbacks: AtomicPtr<WgpuCallbacks>,
}

impl WgpuCallbackRegistry {
    const fn new() -> Self {
        Self {
            callbacks: AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    fn register(
        &self,
        callbacks: &'static WgpuCallbacks,
    ) -> Result<(), WgpuCallbackRegistrationError> {
        let candidate = core::ptr::from_ref(callbacks).cast_mut();
        match self.callbacks.compare_exchange(
            core::ptr::null_mut(),
            candidate,
            Ordering::Release,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(()),
            Err(installed) if core::ptr::eq(installed.cast_const(), callbacks) => Ok(()),
            Err(_) => Err(WgpuCallbackRegistrationError),
        }
    }

    #[inline]
    fn get(&self) -> Option<&'static WgpuCallbacks> {
        let callbacks = self.callbacks.load(Ordering::Acquire);
        if callbacks.is_null() {
            None
        } else {
            // SAFETY: the only stored values come from `register`, whose input
            // is a live `&'static WgpuCallbacks`. The pointer is never replaced
            // or deallocated after successful publication.
            Some(unsafe { &*callbacks })
        }
    }
}

static WGPU_CALLBACKS: WgpuCallbackRegistry = WgpuCallbackRegistry::new();

/// Registers the process-global WGPU staging allocation callbacks.
///
/// The first static pair becomes the permanent process callback contract.
/// Registering that same static pair again is idempotent; a different pair
/// returns [`WgpuCallbackRegistrationError`] and leaves the installed pair
/// unchanged.
///
/// # Errors
///
/// Returns [`WgpuCallbackRegistrationError`] when a different static pair was
/// already registered.
pub fn register_wgpu_callbacks(
    callbacks: &'static WgpuCallbacks,
) -> Result<(), WgpuCallbackRegistrationError> {
    WGPU_CALLBACKS.register(callbacks)
}

#[inline]
fn registered_allocate_callback() -> Option<WgpuAllocateCallback> {
    WGPU_CALLBACKS.get().map(|callbacks| callbacks.allocate)
}

#[inline]
fn registered_deallocate_callback() -> Option<WgpuDeallocateCallback> {
    WGPU_CALLBACKS.get().map(|callbacks| callbacks.deallocate)
}

/// A memory backend that delegates allocation/deallocation to registered callbacks.
/// Used to hook wgpu buffer staging/allocation into Mnemosyne.
pub struct WgpuStagingBackend;

impl MemoryBackend for WgpuStagingBackend {
    const SUPPORTS_PAGE_RESET: bool = false;
    const SUPPORTS_MAKE_GUARD: bool = false;
    const SUPPORTS_DECOMMIT: bool = false;

    #[inline]
    unsafe fn allocate(size: usize) -> *mut u8 {
        if let Some(callback) = registered_allocate_callback() {
            unsafe { callback(size) }
        } else {
            core::ptr::null_mut()
        }
    }

    #[inline]
    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        if let Some(callback) = registered_deallocate_callback() {
            unsafe { callback(ptr, size) }
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use core::alloc::Layout;
    use mnemosyne_core::MemoryBackend;

    use super::{
        WgpuCallbackRegistrationError, WgpuCallbackRegistry, WgpuCallbacks, WgpuStagingBackend,
        register_wgpu_callbacks,
    };

    unsafe extern "C" fn test_alloc(size: usize) -> *mut u8 {
        unsafe { std::alloc::alloc(Layout::from_size_align_unchecked(size, 8)) }
    }

    unsafe extern "C" fn test_dealloc(ptr: *mut u8, size: usize) -> bool {
        unsafe {
            std::alloc::dealloc(ptr, Layout::from_size_align_unchecked(size, 8));
        }
        true
    }

    // SAFETY: the allocator pair uses the same layout contract and neither
    // callback unwinds across the FFI boundary.
    static TEST_CALLBACKS: WgpuCallbacks = unsafe { WgpuCallbacks::new(test_alloc, test_dealloc) };

    unsafe extern "C" fn pair_a_alloc(_size: usize) -> *mut u8 {
        core::ptr::without_provenance_mut(1)
    }

    unsafe extern "C" fn pair_a_dealloc(ptr: *mut u8, _size: usize) -> bool {
        ptr.addr() == 1
    }

    unsafe extern "C" fn pair_b_alloc(_size: usize) -> *mut u8 {
        core::ptr::without_provenance_mut(2)
    }

    unsafe extern "C" fn pair_b_dealloc(ptr: *mut u8, _size: usize) -> bool {
        ptr.addr() == 2
    }

    // SAFETY: each sentinel allocator returns a non-null opaque address that
    // its matching deallocator validates without dereferencing; neither
    // callback unwinds.
    static PAIR_A: WgpuCallbacks = unsafe { WgpuCallbacks::new(pair_a_alloc, pair_a_dealloc) };
    // SAFETY: same contract as `PAIR_A`, with a distinct sentinel address.
    static PAIR_B: WgpuCallbacks = unsafe { WgpuCallbacks::new(pair_b_alloc, pair_b_dealloc) };

    #[test]
    fn typed_callbacks_round_trip_allocation() {
        assert_eq!(register_wgpu_callbacks(&TEST_CALLBACKS), Ok(()));
        assert_eq!(register_wgpu_callbacks(&TEST_CALLBACKS), Ok(()));
        assert_eq!(
            register_wgpu_callbacks(&PAIR_A),
            Err(WgpuCallbackRegistrationError)
        );

        let ptr = unsafe { WgpuStagingBackend::allocate(64) };
        assert!(!ptr.is_null());

        unsafe {
            ptr.write(0x5A);
            assert_eq!(ptr.read(), 0x5A);
        }

        assert!(unsafe { WgpuStagingBackend::deallocate(ptr, 64) });
    }

    #[test]
    fn concurrent_registration_publishes_one_matching_pair() {
        for _ in 0..64 {
            let registry = WgpuCallbackRegistry::new();
            let barrier = std::sync::Barrier::new(3);
            std::thread::scope(|scope| {
                let first = scope.spawn(|| {
                    barrier.wait();
                    registry.register(&PAIR_A)
                });
                let second = scope.spawn(|| {
                    barrier.wait();
                    registry.register(&PAIR_B)
                });
                barrier.wait();

                let first = first.join().expect("first registrar must not panic");
                let second = second.join().expect("second registrar must not panic");
                assert_ne!(first, second, "exactly one distinct pair must win");

                let callbacks = registry
                    .get()
                    .expect("one callback pair must be published after the race");
                let ptr = unsafe { (callbacks.allocate)(8) };
                assert!(
                    unsafe { (callbacks.deallocate)(ptr, 8) },
                    "allocation and deallocation must come from one pair"
                );
            });
        }
    }
}
