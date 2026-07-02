/// Permission identity for the thread allocator that owns a segment.
///
/// This follows the GhostCell separation principle at allocator scale: segment
/// data stores an opaque ownership token, while mutation permission remains with
/// the thread-local allocator that can prove token equality. The representation
/// is a raw pointer-sized value, so checks compile to the same pointer
/// comparison as the previous untyped field.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentOwner(pub usize);

impl SegmentOwner {
    /// No thread currently owns this segment.
    pub const NONE: Self = Self(0);

    /// Builds an owner token from an allocator pointer.
    #[inline(always)]
    pub fn from_ptr<T>(ptr: *mut T) -> Self {
        Self(ptr as usize)
    }

    /// Returns true when this token identifies `ptr`.
    #[inline(always)]
    pub fn matches<T>(self, ptr: *mut T) -> bool {
        self.0 == ptr as usize
    }

    /// Builds an owner token from a thread ID.
    #[inline(always)]
    pub fn from_thread_id(tid: u32) -> Self {
        Self(tid as usize)
    }

    /// Returns true when this token identifies `tid`.
    #[inline(always)]
    pub fn matches_thread_id(self, tid: u32) -> bool {
        self.0 == tid as usize
    }
}

/// Reads the current OS thread id from the Windows TEB.
///
/// On Windows x86-64 the `gs` segment base points at the running thread's TEB
/// and `gs:[0x48]` is the fixed offset of `ClientId.UniqueThread` (the OS
/// thread id). This is the single authoritative TEB thread-id read for the
/// allocator's ownership fast paths, which pair it with
/// [`SegmentOwner::from_thread_id`]/[`SegmentOwner::matches_thread_id`]. It is
/// compiled only on the target where the segment-register load is available;
/// other targets (and Miri) identify ownership by the allocator slot pointer
/// instead, so no portable fallback is defined here.
#[cfg(all(windows, target_arch = "x86_64", not(miri)))]
#[inline(always)]
pub fn current_thread_id() -> u32 {
    let val: u32;
    // SAFETY: On Windows x86_64 the `gs` segment base points at the current
    // thread's TEB, and `gs:[0x48]` is the fixed offset of
    // `ClientId.UniqueThread` (the OS thread id). The read is a single aligned
    // 32-bit load from a thread-local OS structure that is always mapped for a
    // running thread, touches no caller memory, and has no side effects
    // (`nostack`, `readonly`, `preserves_flags`), so it is sound on every
    // running thread.
    unsafe {
        core::arch::asm!(
            "mov {0:e}, gs:[0x48]",
            out(reg) val,
            options(nostack, preserves_flags, readonly)
        );
    }
    val
}

// SAFETY: `SegmentOwner` is a `#[repr(transparent)]` newtype over a plain
// `usize` ownership token (an allocator pointer's address or a thread id). It is
// a value, not a live reference — it confers no access to the pointee and is
// only ever compared for equality (`matches`/`matches_thread_id`), so moving it
// between threads (`Send`) cannot create a data race or dangling access.
unsafe impl Send for SegmentOwner {}
// SAFETY: the token is immutable plain-data (`usize`) read only via `Copy` and
// equality comparison, so shared `&SegmentOwner` access across threads (`Sync`)
// observes no mutation and no aliasing hazard.
unsafe impl Sync for SegmentOwner {}
