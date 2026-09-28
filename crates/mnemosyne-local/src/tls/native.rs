//! Native OS TLS providers using platform APIs and x86_64 TEB ASM.
//!
//! One implementation, two providers: [`NativeOsTls`] routes slot reads and
//! writes through the OS API (`TlsGetValue`/`pthread_getspecific`) and
//! [`AsmTls`] reaches the same slot inline through the Thread Environment Block
//! on Windows x86-64. They differ only in that read/write pair, expressed here
//! as the ZST [`TlsSlotOps`] parameter of a single generic [`OsTlsProvider`] —
//! the same ZST-ops shape the CUDA backends use (`CudaAllocOps`). `AsmTls` is a
//! type alias that falls back to `NativeOsTls` off Windows x86-64, so the five
//! one-line delegations of the former fallback impl are gone.

use super::traits::{TlsProvider, TlsSlotAccess};
use crate::ThreadAllocator;
use crate::tls_slot::LocalAllocatorSlot;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::os_tls::{get_or_init_key, read_value, write_value};

/// The slot read/write pair a native provider uses.
///
/// The sole axis on which the OS-API and TEB-inline providers differ; a ZST so
/// the choice is free and monomorphizes away.
pub trait TlsSlotOps: 'static {
    /// Identifier folded into the provider's `TlsProvider::IDENTIFIER`.
    const IDENTIFIER: &'static str;

    /// Reads this thread's slot `key`.
    ///
    /// # Safety
    ///
    /// `key` must be a valid OS TLS slot index for this process.
    unsafe fn read(key: u32) -> *mut core::ffi::c_void;

    /// Writes `ptr` into this thread's slot `key`.
    ///
    /// # Safety
    ///
    /// `key` must be a valid OS TLS slot index for this process; `ptr` is
    /// stored opaquely and never dereferenced.
    unsafe fn write(key: u32, ptr: *mut core::ffi::c_void);
}

/// Slot ops backed by the OS API (`TlsGetValue`/`TlsSetValue`).
pub struct OsValueOps;

impl TlsSlotOps for OsValueOps {
    const IDENTIFIER: &'static str = "NativeOsTls";

    #[inline(always)]
    unsafe fn read(key: u32) -> *mut core::ffi::c_void {
        read_value(key)
    }

    #[inline(always)]
    unsafe fn write(key: u32, ptr: *mut core::ffi::c_void) {
        write_value(key, ptr);
    }
}

/// Slot ops backed by direct TEB array indexing (Windows x86-64 only).
#[cfg(all(windows, target_arch = "x86_64", not(miri)))]
pub struct TebSlotOps;

#[cfg(all(windows, target_arch = "x86_64", not(miri)))]
impl TlsSlotOps for TebSlotOps {
    const IDENTIFIER: &'static str = "AsmTls";

    #[inline(always)]
    unsafe fn read(key: u32) -> *mut core::ffi::c_void {
        // SAFETY: forwarded — `key` is an OS TLS slot index per the caller's
        // contract.
        unsafe { mnemosyne_core::os_tls::read_teb_slot(key) }
    }

    #[inline(always)]
    unsafe fn write(key: u32, ptr: *mut core::ffi::c_void) {
        // SAFETY: forwarded — `key` is an OS TLS slot index per the caller's
        // contract; the write stores `ptr` opaquely.
        unsafe { mnemosyne_core::os_tls::write_teb_slot(key, ptr) };
    }
}

/// The one native provider, parameterized by its [`TlsSlotOps`].
pub struct OsTlsProvider<B, S, O>(core::marker::PhantomData<(B, S, O)>);

/// OS-API native provider (`TlsGetValue` / `pthread_getspecific`).
pub type NativeOsTls<B, S> = OsTlsProvider<B, S, OsValueOps>;

/// TEB-inline provider on Windows x86-64.
#[cfg(all(windows, target_arch = "x86_64", not(miri)))]
pub type AsmTls<B, S> = OsTlsProvider<B, S, TebSlotOps>;

/// Off Windows x86-64 (or under Miri) `AsmTls` is the OS-API provider.
#[cfg(any(not(all(windows, target_arch = "x86_64")), miri))]
pub type AsmTls<B, S> = NativeOsTls<B, S>;

/// Initializes this thread's slot and returns the allocator pointer.
///
/// The single body behind the lazy-init branch of every provider method; the
/// only variation is whether the thread-exit sentinel is armed
/// (`ARM_THREAD_EXIT`).
#[inline(always)]
fn init_slot<const ARM_THREAD_EXIT: bool, B: HasSegmentPool, S: TlsSlotAccess<B>, O: TlsSlotOps>(
    key: u32,
) -> *mut core::ffi::c_void {
    S::get_slot_standard(|slot| {
        let alloc_ptr = slot.allocator_ptr();
        // SAFETY: `key` came from `get_or_init_key`, so it is a live OS TLS slot
        // index for this process; `O::write` stores `alloc_ptr` opaquely.
        unsafe { O::write(key, alloc_ptr) };
        slot.os_key.set(key);
        if ARM_THREAD_EXIT {
            S::arm_thread_exit(slot);
        }
        alloc_ptr
    })
}

/// Publishes `ptr` into this thread's slot for the `O`/`S` slot ops.
///
/// A private free helper rather than an inline body so the public
/// [`TlsProvider::register_current_allocator_ptr`] forwards its raw pointer to
/// a safe function; that keeps the public trait method clear of
/// `clippy::not_unsafe_ptr_arg_deref` (the write stores the pointer opaquely).
#[inline(always)]
fn publish_ptr<B: HasSegmentPool, S: TlsSlotAccess<B>, O: TlsSlotOps>(ptr: *mut core::ffi::c_void) {
    let Some(key) = get_or_init_key(S::get_os_tls_key()) else {
        return;
    };
    // SAFETY: `key` came from `get_or_init_key`, so it is a live OS TLS slot
    // index for this process; `O::write` stores `ptr` opaquely.
    unsafe { O::write(key, ptr) };
}

impl<B: HasSegmentPool, S: TlsSlotAccess<B>, O: TlsSlotOps> TlsProvider<B>
    for OsTlsProvider<B, S, O>
{
    const IDENTIFIER: &'static str = O::IDENTIFIER;

    #[inline(always)]
    fn register_current_allocator_ptr(ptr: *mut core::ffi::c_void) {
        publish_ptr::<B, S, O>(ptr);
    }

    #[inline(always)]
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        let Some(key) = get_or_init_key(S::get_os_tls_key()) else {
            return S::slot_access_armed(f);
        };
        // SAFETY: `key` is a live OS TLS slot index (`get_or_init_key`); the
        // read yields this thread's own slot value.
        let ptr = unsafe { O::read(key) };
        if !ptr.is_null() {
            // SAFETY: a non-null `ptr` in this thread's OS TLS slot `key` was
            // written by this thread's own `slot.allocator_ptr()` in the init
            // branch below; the slot lives in thread-local storage, so the
            // pointee is exclusively owned by the current thread and no other
            // thread aliases it. `is_allocating` rejects nested same-thread
            // access before a second `&mut` is created.
            // SAFETY: `ptr` is this thread's own slot address (== the allocator
            // address, by the slot's offset-0 invariant) written below.
            unsafe { LocalAllocatorSlot::<B>::with_allocator(ptr, f) }
        } else {
            let alloc_ptr = init_slot::<true, B, S, O>(key);
            // SAFETY: `alloc_ptr` is this slot's own live address.
            unsafe { LocalAllocatorSlot::<B>::with_allocator(alloc_ptr, f) }
        }
    }

    #[inline(always)]
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        let Some(key) = get_or_init_key(S::get_os_tls_key()) else {
            // SAFETY: caller's no-re-entry contract forwarded unchanged.
            return unsafe { S::slot_access_unguarded(f) };
        };
        // SAFETY: `key` is a live OS TLS slot index (`get_or_init_key`).
        let ptr = unsafe { O::read(key) };
        if !ptr.is_null() {
            // SAFETY: `ptr` is this thread's own allocator pointer stored in OS
            // TLS slot `key`; the slot is thread-local, so the pointee is
            // exclusive to the current thread (no cross-thread aliasing).
            // `is_allocating` still gates same-thread re-entry, so no second
            // live `&mut` to the cache can be created. The caller of this
            // `unsafe fn` upholds the no-re-entry contract documented on
            // `with_allocator_unguarded`.
            // SAFETY: as above; the caller upholds the no-re-entry contract.
            unsafe { LocalAllocatorSlot::<B>::with_allocator_unguarded(ptr, f) }
        } else {
            let alloc_ptr = init_slot::<true, B, S, O>(key);
            // SAFETY: `alloc_ptr` is this slot's own live address, and the
            // caller's no-re-entry contract is forwarded unchanged.
            unsafe { LocalAllocatorSlot::<B>::with_allocator_unguarded(alloc_ptr, f) }
        }
    }

    #[inline(always)]
    fn get_allocator_ptr() -> *mut core::ffi::c_void {
        let Some(key) = get_or_init_key(S::get_os_tls_key()) else {
            return S::get_slot_standard(|slot| slot.allocator_ptr());
        };
        // SAFETY: `key` is a live OS TLS slot index (`get_or_init_key`).
        let ptr = unsafe { O::read(key) };
        if !ptr.is_null() {
            ptr
        } else {
            init_slot::<false, B, S, O>(key)
        }
    }

    #[inline(always)]
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
        // SAFETY: `key` is a live OS TLS slot index (`get_or_init_key`); the
        // read yields this thread's cached value without triggering init.
        get_or_init_key(S::get_os_tls_key())
            .map_or(core::ptr::null_mut(), |key| unsafe { O::read(key) })
    }
}
