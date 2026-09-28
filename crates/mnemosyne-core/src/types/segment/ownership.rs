//! [`SegmentOwnership`] — the (owner-token, allocator-cache-ptr) pair embedded
//! in every [`super::Segment`] header.
//!
//! The pair is stored as two atomics rather than two plain fields because the
//! remote free path reads both while the owning thread may be writing them;
//! the `Acquire`/`Release` pairing on these accessors carries the
//! happens-before edge that makes cross-thread free safe.

use crate::types::SegmentOwner;

/// A segment's owner identity: who owns it, and which allocator cache its
/// frees route to.
///
/// The two are published and observed as a pair. A reader that sees an owner
/// and then reads an allocator belonging to the *previous* owner has read a
/// torn identity and will route a free to the wrong cache, which is why the
/// members are private and reachable only through the accessors below.
///
/// Atomics come from [`crate::loom_shim`], so a loom model drives this exact
/// code rather than a transcription of it. That is the point of the type
/// existing separately from `Segment`: a model cannot construct a whole
/// `Segment`, whose `[Page; PAGES_PER_SEGMENT]` would create one instrumented
/// atomic per page, but it can construct this.
pub struct SegmentOwnership {
    owner: crate::loom_shim::AtomicUsize,
    allocator: crate::loom_shim::AtomicPtr<core::ffi::c_void>,
}

impl SegmentOwnership {
    /// An unowned pair.
    ///
    /// `const` in ordinary builds; loom's instrumented atomics are not
    /// const-constructible, so the model build gets a non-const twin.
    #[cfg(not(loom))]
    pub const fn unowned() -> Self {
        Self {
            owner: crate::loom_shim::AtomicUsize::new(SegmentOwner::NONE.0),
            allocator: crate::loom_shim::AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    /// Loom-build constructor. See the `cfg(not(loom))` form above.
    #[cfg(loom)]
    pub fn unowned() -> Self {
        Self {
            owner: crate::loom_shim::AtomicUsize::new(SegmentOwner::NONE.0),
            allocator: crate::loom_shim::AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    /// Reads the owner.
    ///
    /// `Acquire`: pairs with [`Self::set_owner`]'s `Release` so everything the
    /// owner wrote before claiming or orphaning is visible to whoever observes
    /// the identity.
    #[inline(always)]
    pub fn owner(&self) -> SegmentOwner {
        SegmentOwner(self.owner.load(crate::loom_shim::Ordering::Acquire))
    }

    /// Publishes the owner.
    #[inline(always)]
    pub fn set_owner(&self, owner: SegmentOwner) {
        self.owner
            .store(owner.0, crate::loom_shim::Ordering::Release);
    }

    /// Reads the owner's allocator cache.
    #[inline(always)]
    pub fn allocator(&self) -> *mut core::ffi::c_void {
        self.allocator.load(crate::loom_shim::Ordering::Acquire)
    }

    /// Publishes the owner's allocator cache.
    #[inline(always)]
    pub fn set_allocator(&self, allocator: *mut core::ffi::c_void) {
        self.allocator
            .store(allocator, crate::loom_shim::Ordering::Release);
    }
}
