//! Chunk registry: the provenance source for free-path metadata (ADR 0012).
//!
//! A pointer handed to `dealloc` carries the caller's provenance, which may
//! cover only the caller's bytes. Segment headers, page metadata and the huge
//! back-pointer lie outside those bytes, so the free path must not reach them
//! through the caller's pointer (ADR 0009 forbids exposed provenance as the
//! alternative). The registry maps every 2 MiB chunk whose base lies inside a
//! live backend mapping to that mapping's own pointer, and
//! [`locate_segment`](super::locate_segment) rebuilds metadata pointers from
//! it with `with_addr`.
//!
//! # Geometry
//!
//! Two levels: a static root indexed by the high chunk bits, and leaves of
//! `2^LEAF_BITS` donor slots allocated by the caller on first use and never
//! freed. [`VA_BITS`] bounds the addresses a target hands out to user space;
//! registration refuses a mapping beyond it rather than masking.
//!
//! # Atomics
//!
//! The registry uses `core` atomics directly rather than
//! [`loom_shim`](crate::loom_shim): its root is a `static`, which loom's
//! atomics cannot initialize, and no loom model reaches registration.

use crate::constants::SEGMENT_SIZE;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// User virtual-address bits this target hands out without an mmap hint.
///
/// x86-64: 47 on Windows 8.1+ and on Linux unless mmap is hinted above 2^47
/// (Linux `arch/x86/x86_64/5level-paging`, §30.3.2). AArch64 Linux: 48 unless
/// mmap is hinted above 2^48 (`arch/arm64/memory`, "52-bit userspace VAs").
/// 32-bit targets: the whole pointer.
#[cfg(target_pointer_width = "32")]
pub const VA_BITS: u32 = 32;
/// User virtual-address bits this target hands out without an mmap hint.
#[cfg(all(target_pointer_width = "64", target_arch = "x86_64"))]
pub const VA_BITS: u32 = 47;
/// User virtual-address bits this target hands out without an mmap hint.
#[cfg(all(target_pointer_width = "64", not(target_arch = "x86_64")))]
pub const VA_BITS: u32 = 48;

const CHUNK_SHIFT: u32 = SEGMENT_SIZE.trailing_zeros();
const CHUNK_BITS: u32 = VA_BITS - CHUNK_SHIFT;
const LEAF_BITS: u32 = if CHUNK_BITS < 13 { CHUNK_BITS } else { 13 };
const ROOT_BITS: u32 = CHUNK_BITS - LEAF_BITS;
const LEAF_LEN: usize = 1 << LEAF_BITS;
const ROOT_LEN: usize = 1 << ROOT_BITS;

/// Bytes of one registry leaf; the size every `allocate_leaf` call must supply.
pub const LEAF_BYTES: usize = LEAF_LEN * size_of::<AtomicPtr<u8>>();

/// One level-2 table: a donor pointer per chunk, null when unregistered.
#[repr(transparent)]
struct Leaf {
    donors: [AtomicPtr<u8>; LEAF_LEN],
}

static ROOT: [AtomicPtr<Leaf>; ROOT_LEN] =
    [const { AtomicPtr::new(core::ptr::null_mut()) }; ROOT_LEN];

/// Number of leaves installed in [`ROOT`]; leaves are never freed.
static INSTALLED_LEAVES: AtomicUsize = AtomicUsize::new(0);

/// Bytes of registry leaves installed so far.
///
/// Leaves come from the registering caller's backend and live for the rest of
/// the process, so this only grows; it is the registry's share of the
/// backend's mapped bytes.
#[must_use]
pub fn installed_leaf_bytes() -> usize {
    INSTALLED_LEAVES.load(Ordering::Relaxed) * LEAF_BYTES
}

/// Why a mapping could not be registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistrationError {
    /// The mapping ends above `2^VA_BITS`, so its chunks have no root slot.
    BeyondAddressSpace,
    /// `allocate_leaf` returned null.
    LeafUnavailable,
}

/// Chunk indices whose base address lies in `[start, start + len)`.
fn chunk_range(start: usize, len: usize) -> core::ops::Range<usize> {
    let first = start.div_ceil(SEGMENT_SIZE);
    let end = start.saturating_add(len);
    let last_exclusive = end.div_ceil(SEGMENT_SIZE);
    first..last_exclusive
}

/// Registers every chunk whose base lies inside `mapping[..len]`.
///
/// # Errors
///
/// [`RegistrationError::BeyondAddressSpace`] when the mapping ends above
/// `2^VA_BITS`; [`RegistrationError::LeafUnavailable`] when a leaf is needed
/// and `allocate_leaf` returns null. Chunks registered before the failure are
/// unregistered again.
///
/// # Safety
///
/// `mapping` must be the start of a live backend mapping of `len` bytes whose
/// provenance covers the whole mapping, not yet registered. `allocate_leaf`
/// must return null or a writable, `LEAF_BYTES`-long, pointer-aligned region that
/// stays live for the rest of the process; `release_leaf` receives only regions
/// `allocate_leaf` returned that the registry did not install.
pub unsafe fn register_mapping(
    mapping: *mut u8,
    len: usize,
    mut allocate_leaf: impl FnMut() -> *mut u8,
    mut release_leaf: impl FnMut(*mut u8),
) -> Result<(), RegistrationError> {
    let chunks = chunk_range(mapping.addr(), len);
    if chunks.end > ROOT_LEN << LEAF_BITS {
        return Err(RegistrationError::BeyondAddressSpace);
    }
    for chunk in chunks.clone() {
        let slot = &ROOT[chunk >> LEAF_BITS];
        let mut leaf = slot.load(Ordering::Acquire);
        if leaf.is_null() {
            let fresh = allocate_leaf().cast::<Leaf>();
            if fresh.is_null() {
                // SAFETY: the chunks before `chunk` were registered by this call
                // for this same mapping.
                unsafe { unregister_mapping_chunks(chunks.start..chunk) };
                return Err(RegistrationError::LeafUnavailable);
            }
            // SAFETY: `allocate_leaf` returned `LEAF_BYTES` writable bytes; a
            // zeroed leaf is a leaf of null donors.
            unsafe { core::ptr::write_bytes(fresh.cast::<u8>(), 0, LEAF_BYTES) };
            leaf = match slot.compare_exchange(
                core::ptr::null_mut(),
                fresh,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    INSTALLED_LEAVES.fetch_add(1, Ordering::Relaxed);
                    fresh
                }
                Err(installed) => {
                    release_leaf(fresh.cast());
                    installed
                }
            };
        }
        // SAFETY: `leaf` is an installed leaf (never freed) and the index is
        // masked to `LEAF_LEN`.
        unsafe { (*leaf).donors[chunk & (LEAF_LEN - 1)].store(mapping, Ordering::Release) };
    }
    Ok(())
}

/// Unregisters every chunk whose base lies inside `mapping[..len]`.
///
/// # Safety
///
/// `mapping`/`len` must describe a mapping [`register_mapping`] registered, and
/// no free of a block inside it may run concurrently or afterwards.
pub unsafe fn unregister_mapping(mapping: *mut u8, len: usize) {
    // SAFETY: the caller guarantees `mapping`/`len` were registered and that no
    // free inside the mapping runs concurrently or afterwards, which is
    // `unregister_mapping_chunks`'s contract for these chunks.
    unsafe { unregister_mapping_chunks(chunk_range(mapping.addr(), len)) };
}

/// Clears the donor of every chunk in `chunks`.
///
/// # Safety
///
/// Every chunk in `chunks` must be registered, and no free of a block inside
/// those chunks may run concurrently or afterwards.
unsafe fn unregister_mapping_chunks(chunks: core::ops::Range<usize>) {
    for chunk in chunks {
        let leaf = ROOT[chunk >> LEAF_BITS].load(Ordering::Acquire);
        debug_assert!(!leaf.is_null(), "invariant: a registered chunk has a leaf");
        // SAFETY: a non-null root entry is an installed leaf, which is never
        // freed, so the reference is valid for the rest of the process.
        if let Some(leaf) = unsafe { leaf.as_ref() } {
            leaf.donors[chunk & (LEAF_LEN - 1)].store(core::ptr::null_mut(), Ordering::Release);
        }
    }
}

/// The mapping pointer registered for the chunk containing `addr`, or null.
///
/// The returned pointer carries the provenance of the whole mapping, so
/// `donor.with_addr(a)` is valid for any `a` inside it.
#[inline(always)]
#[must_use]
pub fn mapping_donor(addr: usize) -> *mut u8 {
    let chunk = addr >> CHUNK_SHIFT;
    let Some(slot) = ROOT.get(chunk >> LEAF_BITS) else {
        return core::ptr::null_mut();
    };
    let leaf = slot.load(Ordering::Acquire);
    // SAFETY: a non-null root entry is an installed leaf, never freed.
    match unsafe { leaf.as_ref() } {
        Some(leaf) => leaf.donors[chunk & (LEAF_LEN - 1)].load(Ordering::Acquire),
        None => core::ptr::null_mut(),
    }
}
