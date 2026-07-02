use crate::Heap;
use crate::brand::{BrandedBlock, ThreadLocalToken};
use crate::branded_box::BrandedBox;
use core::alloc::Layout;
use core::marker::PhantomData;
use core::ptr::NonNull;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::LocalAllocatorSelector;
use mnemosyne_local::internal::HasSegmentPool;

pub mod ops;
pub mod traits;

/// A dynamically growing array allocated from a `Heap`.
///
/// Automatically handles growth and reallocation, dropping all elements on drop.
pub struct BrandedVec<
    'brand,
    'heap,
    T,
    P: AllocPolicy = mnemosyne_core::StandardPolicy,
    B: HasSegmentPool + LocalAllocatorSelector<B> = mnemosyne_backend::MemoryBackendWrapper,
> {
    pub(crate) ptr: NonNull<T>,
    pub(crate) cap: usize,
    pub(crate) len: usize,
    pub(crate) heap: &'heap Heap<'brand, P, B>,
    pub(crate) _non_send_sync: core::marker::PhantomData<*mut ()>,
}

impl<'brand, 'heap, T, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    BrandedVec<'brand, 'heap, T, P, B>
{
    /// Creates a new empty `BrandedVec` backed by the given `Heap`.
    #[inline(always)]
    pub fn new(heap: &'heap Heap<'brand, P, B>) -> Self {
        Self {
            ptr: NonNull::dangling(),
            cap: if core::mem::size_of::<T>() == 0 {
                usize::MAX
            } else {
                0
            },
            len: 0,
            heap,
            _non_send_sync: core::marker::PhantomData,
        }
    }

    /// Creates a new `BrandedVec` with space for at least `capacity` elements.
    #[inline]
    pub fn with_capacity(
        heap: &'heap Heap<'brand, P, B>,
        token: &ThreadLocalToken<'brand>,
        capacity: usize,
    ) -> Option<Self> {
        if capacity == 0 || core::mem::size_of::<T>() == 0 {
            return Some(Self::new(heap));
        }
        let layout = Layout::array::<T>(capacity).ok()?;
        let block = heap.alloc(token, layout)?;
        Some(Self {
            ptr: block.ptr.cast(),
            cap: capacity,
            len: 0,
            heap,
            _non_send_sync: core::marker::PhantomData,
        })
    }

    /// Converts this vector into a boxed slice, shrinking the memory allocation to fit.
    #[inline]
    pub fn into_boxed_slice(
        mut self,
        token: &mut ThreadLocalToken<'brand>,
    ) -> BrandedBox<'brand, 'heap, [T], P, B> {
        if core::mem::size_of::<T>() == 0 {
            // SAFETY: `T` is zero-sized, so a `[T]` of any length occupies no
            // bytes; `NonNull::dangling()` is a valid, aligned base for a
            // zero-sized slice of `self.len` ZST elements. `from_raw_parts_mut`
            // requires the pointer be non-null and aligned (dangling satisfies
            // both for a ZST) and the resulting fat pointer is never read/written
            // for storage. The base pointer is non-null, so `new_unchecked` is
            // sound. Ownership of the `len` logical elements transfers to the
            // returned `BrandedBox` (`self` is forgotten below), so no element is
            // dropped twice.
            let slice_ptr = unsafe {
                let raw_slice =
                    core::slice::from_raw_parts_mut(NonNull::<T>::dangling().as_ptr(), self.len);
                NonNull::new_unchecked(raw_slice)
            };
            let heap = self.heap;
            core::mem::forget(self);
            return BrandedBox {
                ptr: slice_ptr,
                heap,
                _non_send_sync: core::marker::PhantomData,
            };
        }

        // Best-effort shrink to fit via the shared SSOT helper; a failed realloc
        // leaves the (larger) block in place and the boxed slice still owns it
        // correctly, so the `Err` is intentionally ignored here.
        let _ = self.shrink_to_len(token);

        // SAFETY: for non-ZST `T`, `self.ptr` addresses a live allocation of at
        // least `self.len` initialized `T` (after the shrink above, `self.cap`
        // is either unchanged or equal to `self.len`, and `[0, self.len)` is
        // always the initialized prefix). `slice_from_raw_parts_mut` builds a fat
        // pointer over exactly those `self.len` elements; `self.ptr` is non-null
        // (`NonNull`), so `new_unchecked` is sound. Ownership of the elements and
        // the backing block transfers to the returned `BrandedBox` (`self` is
        // forgotten below), so the block is freed exactly once.
        let slice_ptr = unsafe {
            let raw_slice = core::ptr::slice_from_raw_parts_mut(self.ptr.as_ptr(), self.len);
            NonNull::new_unchecked(raw_slice)
        };

        let heap = self.heap;
        core::mem::forget(self);

        BrandedBox {
            ptr: slice_ptr,
            heap,
            _non_send_sync: core::marker::PhantomData,
        }
    }

    /// Converts a `BrandedBox<'brand, 'heap, [T], P, B>` back into a `BrandedVec<'brand, 'heap, T, P, B>`.
    ///
    /// This does not allocate or copy.
    #[inline]
    pub fn from_boxed_slice(boxed_slice: BrandedBox<'brand, 'heap, [T], P, B>) -> Self {
        let len = boxed_slice.len();
        let heap = boxed_slice.heap;
        let block = boxed_slice.into_raw();
        Self {
            // SAFETY: `block.ptr` originates from a `BrandedBox<[T]>`'s
            // `NonNull<[T]>` and is therefore non-null; reinterpreting the slice
            // base address as the element pointer `*mut T` preserves
            // non-nullness (and, for non-ZST `T`, the original allocation's
            // alignment for `T`), so `new_unchecked` is sound. Ownership of the
            // block transfers from the consumed box to the new vector with no
            // copy.
            ptr: unsafe { NonNull::new_unchecked(block.ptr.as_ptr() as *mut T) },
            cap: if core::mem::size_of::<T>() == 0 {
                usize::MAX
            } else {
                len
            },
            len,
            heap,
            _non_send_sync: core::marker::PhantomData,
        }
    }

    /// Grows the backing allocation to exactly `new_cap` elements and updates
    /// `ptr`/`cap` on success. This is the single authoritative grow path shared
    /// by [`push`](BrandedVec::push) and [`reserve`](BrandedVec::reserve), so the
    /// alloc-when-empty / realloc-otherwise mechanics cannot drift between them;
    /// each caller keeps only its own capacity *policy* (push's initial-4
    /// doubling vs reserve's `max(cap*2, needed)`).
    ///
    /// Callers guarantee `T` is non-ZST and `new_cap > self.cap`. On layout
    /// overflow or allocation failure the vector is left unchanged and `Err(())`
    /// is returned.
    #[inline]
    fn grow_to(&mut self, token: &mut ThreadLocalToken<'brand>, new_cap: usize) -> Result<(), ()> {
        let new_layout = Layout::array::<T>(new_cap).map_err(|_| ())?;
        if self.cap == 0 {
            let block = self.heap.alloc(token, new_layout).ok_or(())?;
            self.ptr = block.ptr.cast();
        } else {
            let old_layout = Layout::array::<T>(self.cap).unwrap_or_else(|_| {
                debug_assert!(false, "Layout array calculation failed for valid capacity");
                // SAFETY: this branch is reached only when `self.cap != 0`, which
                // means a `Layout::array::<T>(self.cap)` already succeeded at the
                // prior allocation site; recomputing the identical layout cannot
                // fail, so the `Err` arm is unreachable.
                unsafe { core::hint::unreachable_unchecked() }
            });
            let block = BrandedBlock {
                ptr: self.ptr,
                _marker: PhantomData,
            };
            let new_block = self
                .heap
                .realloc(token, block, old_layout, new_layout.size())
                .ok_or(())?;
            self.ptr = new_block.ptr.cast();
        }
        self.cap = new_cap;
        Ok(())
    }

    /// Reserves capacity for at least `additional` more elements to be inserted in the vector.
    ///
    /// # Errors
    /// Returns `Err(())` if layout calculations overflow or allocation fails.
    #[inline]
    #[allow(clippy::result_unit_err)]
    pub fn reserve(
        &mut self,
        token: &mut ThreadLocalToken<'brand>,
        additional: usize,
    ) -> Result<(), ()> {
        if core::mem::size_of::<T>() == 0 {
            return Ok(());
        }
        let needed = match self.len.checked_add(additional) {
            Some(n) => n,
            None => return Err(()),
        };
        if needed <= self.cap {
            return Ok(());
        }
        let new_cap = core::cmp::max(self.cap.checked_mul(2).unwrap_or(needed), needed);
        self.grow_to(token, new_cap)
    }

    /// Shrinks the capacity of the vector as much as possible.
    ///
    /// # Errors
    /// Returns `Err(())` if allocation fails.
    #[inline]
    #[allow(clippy::result_unit_err)]
    pub fn shrink_to_fit(&mut self, token: &mut ThreadLocalToken<'brand>) -> Result<(), ()> {
        if core::mem::size_of::<T>() == 0 {
            return Ok(());
        }
        self.shrink_to_len(token)
    }

    /// Shrinks the backing allocation so its capacity equals `self.len` — the
    /// single authoritative shrink path shared by
    /// [`shrink_to_fit`](BrandedVec::shrink_to_fit) and
    /// [`into_boxed_slice`](BrandedVec::into_boxed_slice), so the
    /// free-when-empty / realloc-to-len mechanics cannot drift between them.
    ///
    /// Callers guarantee `T` is non-ZST. A no-op when `cap <= len`; frees the
    /// block when `len == 0`; otherwise reallocates down to `len` elements.
    /// Returns `Err(())` only if the shrinking realloc fails, leaving the vector
    /// valid and unchanged (the over-sized block is retained).
    #[inline]
    fn shrink_to_len(&mut self, token: &mut ThreadLocalToken<'brand>) -> Result<(), ()> {
        if self.cap <= self.len {
            return Ok(());
        }
        if self.len == 0 {
            // SAFETY: reached only with non-ZST `T` and `self.cap > self.len == 0`,
            // so `self.cap > 0` and `self.ptr` is a live block from `self.heap`
            // (not the dangling sentinel). No element is initialized, so freeing
            // drops nothing; `self.ptr`/`self.cap` reset to the dangling sentinel
            // right after, so the freed block is never reused.
            unsafe {
                self.heap.free_raw(self.ptr.as_ptr() as *mut u8);
            }
            self.ptr = NonNull::dangling();
            self.cap = 0;
            return Ok(());
        }
        let old_layout = Layout::array::<T>(self.cap).map_err(|_| ())?;
        let block = BrandedBlock {
            ptr: self.ptr,
            _marker: PhantomData,
        };
        let new_size = core::mem::size_of::<T>() * self.len;
        match self.heap.realloc(token, block, old_layout, new_size) {
            Some(new_block) => {
                self.ptr = new_block.ptr.cast();
                self.cap = self.len;
                Ok(())
            }
            None => Err(()),
        }
    }
}
