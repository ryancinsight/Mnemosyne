//! The [`Segment`] struct shape, its `Send`/`Sync` impls, `Default`, and
//! the `initialize` / `huge_mapping_suffix_from` lifecycle methods.

use super::SegmentOwnership;
use crate::constants::{PAGE_SIZE, PAGES_PER_SEGMENT};
use crate::types::Page;

// The key mask is a repeated `01` bit pattern. Deriving it from the pointer
// width keeps segment initialization valid for both 32-bit WASM and 64-bit
// desktop targets without an overflowing literal.
const PAGE_KEY_MASK: usize = usize::MAX / 3;

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
    pub(super) is_current: bool,
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

impl Default for Segment {
    /// Fresh segments default to the standard unencrypted free-list mode.
    ///
    /// This matches the runtime guard used by `free_list_mode_matches`: until a
    /// thread claims a segment for a hardened policy and keys it, the segment is
    /// in the zero-cost default mode and must only be checked against `false`.
    #[inline]
    fn default() -> Self {
        Self {
            raw_alloc_ptr: core::ptr::null_mut(),
            ownership: SegmentOwnership::unowned(),
            is_current: false,
            next_owned_segment: core::ptr::null_mut(),
            prev_owned_segment: core::ptr::null_mut(),
            next_free_segment: crate::loom_shim::AtomicPtr::new(core::ptr::null_mut()),
            free_list_encrypted: false,
            numa_node: 0,
            page_occupied_mask: 0,
            page_linked_mask: 0,
            keys: [0; PAGES_PER_SEGMENT],
            // `from_fn` rather than an inline const block: under `cfg(loom)`
            // `Page::new` is not `const` (loom's atomics have no const
            // constructor), and this is a cold constructor used only by the
            // test fixture -- real segments are built by `initialize`.
            pages: core::array::from_fn(|_| Page::new()),
        }
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
                    (aligned_ptr as usize).wrapping_add(i * PAGE_SIZE) ^ PAGE_KEY_MASK;
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
        let huge_size = self.pages[0].block_size as usize;
        debug_assert!(
            huge_size > 0,
            "huge_mapping_suffix_from called on a segment whose pages[0].block_size is zero"
        );
        let raw_ptr_addr = self.raw_alloc_ptr as usize;
        let mapping_end = raw_ptr_addr
            .checked_add(huge_size)
            .expect("raw_alloc_ptr + huge_size overflowed the address space");
        let user_addr = user_ptr as usize;
        debug_assert!(
            user_addr >= raw_ptr_addr,
            "user_ptr {:p} precedes raw_alloc_ptr {:p}",
            user_ptr,
            self.raw_alloc_ptr
        );
        debug_assert!(
            user_addr <= mapping_end,
            "user_ptr {:p} past mapping end (raw_alloc_ptr {:p}, size {})",
            user_ptr,
            self.raw_alloc_ptr,
            huge_size
        );
        mapping_end
            .checked_sub(user_addr)
            .expect("user_ptr is outside the mapping interval")
    }
}
