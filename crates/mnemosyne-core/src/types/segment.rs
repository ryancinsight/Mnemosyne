//! Segment metadata: the fixed-size mapping that owns a run of pages.

use crate::constants::{PAGE_SIZE, PAGES_PER_SEGMENT};
use crate::types::{Page, SegmentOwner};

/// Metadata representing a segment of memory.
///
/// A segment is a large, aligned virtual memory allocation (typically 2MB).
pub struct Segment {
    /// The original raw allocation pointer returned by the OS.
    ///
    /// Used for tracking and deallocation since OS allocators might require
    /// the original unaligned pointer.
    pub raw_alloc_ptr: *mut u8,
    /// Permission identity for the owner ThreadAllocator cache.
    ///
    /// Atomic because it is genuinely shared: the owning thread writes it when
    /// it claims or orphans the segment, while *remote* threads read it during
    /// cross-thread free to decide routing. As a plain `usize` that pairing was
    /// Who owns this segment, and the allocator cache to route its frees to.
    ///
    /// One field rather than two because the pair is only ever meaningful
    /// together: an observer that reads an owner and then reads a stale or
    /// absent allocator for it has read a torn identity. Keeping them as
    /// independent members made that tearing expressible; keeping them in one
    /// unit with private members means the only way to touch either is through
    /// the paired accessors, which carry the Release/Acquire pairing.
    ///
    /// It is also what makes the protocol model-checkable. A loom model cannot
    /// build a whole `Segment` — the embedded `[Page; PAGES_PER_SEGMENT]` would
    /// create one instrumented atomic per page — but it can build this.
    pub ownership: SegmentOwnership,
    /// True while this segment is the owner's active page-slicing segment.
    ///
    /// Deliberately non-atomic, and private so that stays true: this is the one
    /// header field written after publication that no remote thread reads. The
    /// cross-thread free path (`thread_free_cold`) pushes into the page's
    /// `AtomicFreeList` and touches nothing else in the header, and every
    /// reader of this flag — the occupancy transitions, the local free fast
    /// path, the defragmentation sweep — runs on the owning thread, most of
    /// them already holding `&mut ThreadAllocator`.
    ///
    /// Making it atomic would buy nothing and cost something: it sits on the
    /// small-free fast path, and an atomic here would advertise a cross-thread
    /// contract that does not exist, inviting exactly the remote access the
    /// protocol forbids. What was missing was enforcement rather than
    /// synchronization, so the field is private and reached only through
    /// [`Segment::is_current`] / [`Segment::set_current`], whose `# Safety`
    /// contracts state the owner-only requirement.
    ///
    /// The flag must be false whenever a segment crosses back to a global pool,
    /// or the next thread to claim it would inherit a stale "currently being
    /// sliced" state and skip occupancy bookkeeping for it. `reclaim_owned_segments`
    /// upholds this by clearing the allocator's current segment before it walks
    /// the owned chain and clearing the flag again on every node it orphans;
    /// the defragmentation sweep upholds it by skipping the current segment
    /// entirely.
    is_current: bool,
    /// Pointer to the next segment owned by the same ThreadAllocator.
    pub next_owned_segment: *mut Segment,
    /// Pointer to the previous segment owned by the same ThreadAllocator.
    ///
    /// The owned-segments list is intrusive and doubly linked so a thread can
    /// splice any owned segment out in O(1) during `try_reclaim_segment`
    /// without searching for its predecessor. `Segment` metadata is multiple
    /// kilobytes (it embeds the `[Page; PAGES_PER_SEGMENT]` array), so the
    /// extra back-pointer carries no cache-line cost on the allocation hot
    /// path, which never touches this field.
    pub prev_owned_segment: *mut Segment,
    /// Pointer to the next free segment in the global pool.
    ///
    /// Atomic because the pool stack's `pop` genuinely races on it. The winning
    /// popper clears this link after its CAS, on the reasoning that the CAS made
    /// it the exclusive owner — but that only excludes threads reading the head
    /// *after* the CAS. A popper that read the same head before it still holds
    /// the node pointer and can read this field while the winner writes it. As a
    /// plain `*mut`, that is a data race and undefined behaviour, benign as the
    /// generated code may be; loom reports it as a causality violation
    /// (`mnemosyne-arena`'s `loom_tagged_stack`, MN-455).
    ///
    /// `Relaxed` everywhere is sufficient and is what every site uses. This link
    /// carries no happens-before obligation of its own: publication is the head
    /// CAS's `Release`, and observation is its `Acquire`, so a node reached
    /// through the head already synchronizes with the push that linked it. The
    /// atomic here is for the absence of a race, not for ordering.
    pub next_free_segment: crate::loom_shim::AtomicPtr<Segment>,
    /// If true, free list pointers in this segment are XOR-encrypted.
    pub free_list_encrypted: bool,
    /// NUMA node ID where this segment was allocated.
    pub numa_node: u32,
    /// Mask tracking pages with active allocations.
    ///
    /// The current slicing segment may retain bits for pages that have
    /// returned to zero live allocations. Defragmentation skips the current
    /// segment, and later sweeps validate `alloc_count`, so the mask remains a
    /// conservative reclaim accelerator rather than an ownership authority.
    pub page_occupied_mask: u32,
    /// Mask tracking pages currently linked in the allocator's lists (active, full, empty).
    pub page_linked_mask: u32,
    /// Per-page keys for free-list pointer encryption.
    pub keys: [usize; PAGES_PER_SEGMENT],
    /// The pages metadata array. Page 0 is reserved for segment metadata.
    pub pages: [Page; PAGES_PER_SEGMENT],
}

// SAFETY: `Segment` is a metadata header whose raw pointer fields
// (`raw_alloc_ptr`, `owner_allocator`, the intrusive list links) and interior
// mutability are gated by the segment-ownership protocol: a segment carries an
// opaque `owner` token, and only the thread allocator that can prove token
// equality (`Segment::owner` + `SegmentOwner::matches`) mutates its fields, while
// cross-thread frees route through each page's `AtomicFreeList`. No field is
// thread-affine, so transferring ownership of a `Segment` header between
// threads (`Send`) is sound once the previous owner has released it.
unsafe impl Send for Segment {}
// SAFETY: the cross-thread-reachable state is synchronized: each page's
// `AtomicFreeList`, and the `owner` / `owner_allocator` identity pair, are
// atomic. `free_list_encrypted` and the per-page `keys` are written only during
// initialization, before the segment is published to any other thread.
//
// The previous justification here claimed that "all non-atomic fields are
// mutated solely by the proven owner ... so a shared reference observes no data
// race". That was false in both halves, and Miri contradicted it: the owner
// mutated `owner`/`owner_allocator`/`is_current` *while* remote threads read
// the header on the cross-thread free path, and forming a shared reference at
// all retags the whole `Segment`, so it races with any concurrent field write
// regardless of which field the reader wanted. That is why the accessors here
// take `*const Segment` and project to one field rather than taking `&self`.
//
// `is_current` remains non-atomic and is still written only by the owner. That
// is now enforced rather than asserted: the field is private and its accessors
// carry an owner-only `# Safety` contract, so no site outside this module can
// reach it and no reader can acquire it through a whole-header reference.
unsafe impl Sync for Segment {}

/// Recovers the parent segment header and page index for a user pointer.
///
/// Every small allocation lives inside a `SEGMENT_ALIGN`-aligned segment, so
/// masking `ptr` down to `SEGMENT_SIZE` yields the segment header, and the
/// mid-address `PAGE_SHIFT` bits (masked by `PAGES_PER_SEGMENT - 1`) yield the
/// page index. This is the single authoritative pointer→(segment, page_index)
/// classifier shared by the free, realloc, and usable-size fast paths.
///
/// # Safety
///
/// `ptr` must be a non-null pointer returned by a Mnemosyne small/huge
/// allocation, so the recovered segment header is live and the page index is a
/// valid index into its `pages` array.
#[inline(always)]
pub unsafe fn locate_segment(ptr: *mut u8) -> (*mut Segment, usize) {
    let ptr_val = ptr.addr();
    let segment_addr = ptr_val & !(crate::constants::SEGMENT_SIZE - 1);
    let segment = ptr.map_addr(|_| segment_addr).cast::<Segment>();
    let page_index = (ptr_val >> crate::constants::PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    (segment, page_index)
}

/// Recovers a page-metadata pointer without borrowing the enclosing mapping.
///
/// Mnemosyne stores allocator metadata and user blocks in one OS mapping.
/// Cached intrusive-list pointers therefore cross user alloc/free calls that
/// may invalidate reference-derived provenance. Metadata access must re-create
/// provenance from the live mapping address instead of retaining a tag from an
/// earlier `&mut Segment::pages` projection.
///
/// # Safety
///
/// `segment` must identify a live initialized segment and `page_index` must be
/// less than `PAGES_PER_SEGMENT`.
#[inline(always)]
pub unsafe fn locate_page(segment: *mut Segment, page_index: usize) -> *mut Page {
    debug_assert!(page_index < PAGES_PER_SEGMENT);
    // SAFETY: the caller guarantees a live segment and an in-range index. A
    // raw place projection retains the segment allocation's provenance and
    // creates no reference to the enclosing metadata.
    unsafe { &raw mut (*segment).pages[page_index] }
}

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

impl Segment {
    /// Initializes a segment header at a given aligned address.
    ///
    /// # Safety
    ///
    /// `aligned_ptr` must be aligned to `SEGMENT_ALIGN` and valid for write.
    pub unsafe fn initialize(aligned_ptr: *mut Segment, raw_alloc_ptr: *mut u8, numa_node: u32) {
        // SAFETY: aligned_ptr must point to a valid, exclusive, aligned memory segment.
        // We initialize the segment fields and establish parent/child pointers safely.
        unsafe {
            let segment = &mut *aligned_ptr;
            segment.raw_alloc_ptr = raw_alloc_ptr;
            (*core::ptr::addr_of_mut!(segment.ownership)) = SegmentOwnership::unowned();
            segment.is_current = false;
            segment.next_owned_segment = core::ptr::null_mut();
            segment.prev_owned_segment = core::ptr::null_mut();
            (*core::ptr::addr_of_mut!(segment.next_free_segment)) =
                crate::loom_shim::AtomicPtr::new(core::ptr::null_mut());
            segment.free_list_encrypted = false;
            segment.numa_node = numa_node;
            segment.page_occupied_mask = 0;
            segment.page_linked_mask = 0;
            // Page 0 holds segment metadata and is never allocated from;
            // only pages 1..PAGES_PER_SEGMENT need explicit free-list state.
            // We still initialize page 0 with `Page::new()` so debugging and
            // memory-tracing tools observe uniform metadata across the
            // whole array. No page stores a back-pointer to the segment
            // because every caller recovers it by rounding the page address
            // down to `SEGMENT_ALIGN`.
            for i in 0..PAGES_PER_SEGMENT {
                segment.keys[i] =
                    (aligned_ptr as usize).wrapping_add(i * PAGE_SIZE) ^ 0x5555555555555555;
                segment.pages[i] = Page::new();
                segment.pages[i].page_index = i as u8;
            }
        }
    }

    /// Returns the byte distance from `user_ptr` to the end of the OS-side
    /// mapping for a huge allocation owned by this segment header.
    ///
    /// The mapping starts at `self.raw_alloc_ptr` and has length
    /// `self.pages[0].block_size` (set to `total_alloc_size` by
    /// `allocate_large_or_huge`). Callers that need the usable suffix of a
    /// huge allocation — `usable_size`, the `SecurePolicy` poisoning
    /// sizing, any future bounds-aware huge-alloc accessor — must use
    /// this helper instead of computing `(self as usize) + block_size -
    /// user_ptr`, because the segment header sits at `aligned_addr =
    /// align_up(raw_alloc_ptr, SEGMENT_ALIGN)`, which can be up to
    /// `SEGMENT_ALIGN - 1` bytes past `raw_alloc_ptr`. Using the
    /// segment header as the base would over-report by exactly that
    /// offset and walk callers past the OS mapping boundary.
    ///
    /// # Safety
    ///
    /// `self` must be a segment header initialized by `Segment::initialize`
    /// for a *huge* allocation (`pages[0].block_size > 0`). `user_ptr`
    /// must lie within `[raw_alloc_ptr, raw_alloc_ptr + block_size)`.
    #[inline]
    pub unsafe fn huge_mapping_suffix_from(&self, user_ptr: *const u8) -> usize {
        let huge_size = self.pages[0].block_size;
        debug_assert!(
            huge_size > 0,
            "huge_mapping_suffix_from called on a segment whose pages[0].block_size is zero"
        );
        let raw_ptr_addr = self.raw_alloc_ptr as usize;
        debug_assert!(
            user_ptr as usize >= raw_ptr_addr,
            "user_ptr {:p} precedes raw_alloc_ptr {:p}",
            user_ptr,
            self.raw_alloc_ptr
        );
        debug_assert!(
            user_ptr as usize <= raw_ptr_addr + huge_size,
            "user_ptr {:p} past mapping end (raw_alloc_ptr {:p}, size {})",
            user_ptr,
            self.raw_alloc_ptr,
            huge_size
        );
        (raw_ptr_addr + huge_size) - user_ptr as usize
    }

    /// Returns the free-list encryption cookie for page `page_index` under a
    /// runtime encryption flag: the per-page key when `encrypted`, else `0`.
    ///
    /// This is the single authoritative cookie accessor; the free, realloc,
    /// pop, reclaim, and initialization paths route their `if encrypted {
    /// keys[i] } else { 0 }` selection through it (or the `P`-generic
    /// [`Segment::cookie_for`]) instead of indexing `keys` inline.
    ///
    /// # Safety
    ///
    /// `self` must be this page's parent segment header and `page_index` must be
    /// a valid index into `keys` (`< PAGES_PER_SEGMENT`).
    #[inline(always)]
    pub unsafe fn cookie_for_dynamic(
        segment: *const Segment,
        encrypted: bool,
        page_index: usize,
    ) -> usize {
        if encrypted {
            debug_assert!(page_index < PAGES_PER_SEGMENT);
            // Projected rather than reached through `&self`: this runs on the
            // cross-thread free path, where the owning thread may concurrently
            // write other segment fields. A reference retags the *whole*
            // `Segment`, which races with those writes — Miri reported exactly
            // that against `owner_allocator`. Touching only `keys` does not.
            //
            // SAFETY: the caller's contract guarantees `segment` is the valid
            // parent header and `page_index` is in range.
            let keys = unsafe { &raw const (*segment).keys };
            // SAFETY: `keys` addresses the initialized per-page key array and
            // `page_index` is in range.
            unsafe { *(*keys).get_unchecked(page_index) }
        } else {
            0
        }
    }

    /// Returns the free-list encryption cookie for page `page_index` under the
    /// compile-time policy `P`: the per-page key when `P` encrypts, else `0`.
    ///
    /// The const `P::ENABLE_FREE_LIST_ENCRYPTION` const-propagates into
    /// [`Segment::cookie_for_dynamic`], so the branch resolves at compile time.
    ///
    /// Debug builds additionally enforce that the static policy agrees with
    /// this segment's recorded mode. This remains a diagnostic guard for
    /// lower-level direct `ThreadAllocator` callers; the public thread-local
    /// selector prevents the mismatch by assigning each mode its own cache.
    /// Release builds compile the check out.
    ///
    /// # Safety
    ///
    /// Same contract as [`Segment::cookie_for_dynamic`].
    #[inline(always)]
    pub unsafe fn cookie_for<P: crate::policy::AllocPolicy>(
        segment: *const Segment,
        page_index: usize,
    ) -> usize {
        // SAFETY: caller guarantees a valid header; reading one field by
        // projection avoids retagging the whole segment.
        debug_assert_eq!(
            unsafe { Self::free_list_encrypted(segment) },
            P::ENABLE_FREE_LIST_ENCRYPTION,
            "free-list mode mismatch: policy vs segment (ADR 0001)"
        );
        // SAFETY: forwarded unchanged from this method's `# Safety` contract.
        unsafe { Self::cookie_for_dynamic(segment, P::ENABLE_FREE_LIST_ENCRYPTION, page_index) }
    }

    /// Reads the segment's free-list encryption mode.
    ///
    /// Raw-pointer form for the same reason as the other accessors here: this
    /// runs on the cross-thread free path, and a `&Segment` retags the whole
    /// header, racing with the owner's concurrent writes to unrelated fields.
    /// The field itself is only written during initialization, before the
    /// segment is published, so a plain read of it is sound once the retag is
    /// avoided.
    ///
    /// # Safety
    ///
    /// `segment` must point to a live segment header.
    #[inline(always)]
    pub unsafe fn free_list_encrypted(segment: *const Segment) -> bool {
        // SAFETY: caller guarantees a live header; the projection touches only
        // this field.
        unsafe { (*segment).free_list_encrypted }
    }

    /// Reads the cached owner-allocator pointer.
    ///
    /// Raw-pointer form and `Acquire` for the same reasons as
    /// [`Segment::owner`]: a `&self` accessor would retag the whole segment,
    /// and this read must observe the state the owner published.
    ///
    /// # Safety
    ///
    /// `segment` must point to a live segment header.
    #[inline(always)]
    pub unsafe fn owner_allocator(segment: *const Segment) -> *mut core::ffi::c_void {
        // SAFETY: caller guarantees a live header; the projection touches only
        // the atomic field.
        let field = unsafe { &raw const (*segment).ownership };
        // SAFETY: `field` addresses the initialized ownership pair.
        unsafe { (*field).allocator() }
    }

    /// Publishes the cached owner-allocator pointer.
    ///
    /// # Safety
    ///
    /// `segment` must point to a live segment header.
    #[inline(always)]
    pub unsafe fn set_owner_allocator(segment: *const Segment, allocator: *mut core::ffi::c_void) {
        // SAFETY: caller guarantees a live header.
        let field = unsafe { &raw const (*segment).ownership };
        // SAFETY: `field` addresses the initialized ownership pair.
        unsafe { (*field).set_allocator(allocator) };
    }

    /// Reads the active-slicing flag.
    ///
    /// Takes a raw pointer for the same reason as [`Segment::owner`]: a `&self`
    /// would retag the whole header and race with any concurrent write to any
    /// other field. The projection touches this one byte.
    ///
    /// # Safety
    ///
    /// `segment` must point to a live segment header, and the caller must be
    /// that segment's owning thread. This field is not synchronized; reading it
    /// from a non-owner thread is a data race.
    #[inline(always)]
    pub unsafe fn is_current(segment: *const Segment) -> bool {
        // SAFETY: caller guarantees a live header owned by this thread; the
        // projection reads only the `is_current` byte.
        let field = unsafe { &raw const (*segment).is_current };
        // SAFETY: `field` addresses the initialized flag.
        unsafe { field.read() }
    }

    /// Sets or clears the active-slicing flag.
    ///
    /// # Safety
    ///
    /// Carries [`Segment::is_current`]'s contract: live header, owning thread.
    /// Writing from a non-owner thread is a data race.
    #[inline(always)]
    pub unsafe fn set_current(segment: *mut Segment, value: bool) {
        // SAFETY: caller guarantees a live header owned by this thread; the
        // projection writes only the `is_current` byte.
        let field = unsafe { &raw mut (*segment).is_current };
        // SAFETY: `field` addresses the initialized flag.
        unsafe { field.write(value) };
    }

    /// Reads the segment's owner identity.
    ///
    /// Takes a raw pointer rather than `&self` on purpose: a reference retags
    /// the *whole* `Segment`, which races against any concurrent write to any
    /// other field (Miri caught exactly that against `is_current`). Projecting
    /// to the single atomic field touches only that field.
    ///
    /// `Acquire`: a remote thread reading this to route a cross-thread free must
    /// see the segment state published by the owner's `Release` write in
    /// [`Segment::set_owner`].
    ///
    /// # Safety
    ///
    /// `segment` must point to a live segment header.
    #[inline(always)]
    pub unsafe fn owner(segment: *const Segment) -> SegmentOwner {
        // SAFETY: caller guarantees a live header; the projection touches only
        // the `owner` field.
        let field = unsafe { &raw const (*segment).ownership };
        // SAFETY: `field` addresses the initialized ownership pair.
        unsafe { (*field).owner() }
    }

    /// Publishes a new owner identity for this segment.
    ///
    /// `Release`: pairs with [`Segment::owner`]'s `Acquire` so everything the
    /// owner wrote before claiming or orphaning the segment is visible to the
    /// remote thread that observes the new identity. Raw-pointer form for the
    /// same whole-struct-retag reason as the reader.
    ///
    /// # Safety
    ///
    /// `segment` must point to a live segment header.
    #[inline(always)]
    pub unsafe fn set_owner(segment: *const Segment, owner: SegmentOwner) {
        // SAFETY: caller guarantees a live header.
        let field = unsafe { &raw const (*segment).ownership };
        // SAFETY: `field` addresses the initialized ownership pair.
        unsafe { (*field).set_owner(owner) };
    }
}
