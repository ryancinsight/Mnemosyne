use crate::raw_heap::RawHeap;
use crate::Heap;
use core::marker::PhantomData;
use core::ptr::NonNull;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::internal::HasSegmentPool;
use mnemosyne_local::LocalAllocatorSelector;

use melinoe::sync::thread_local_scope;

// Brand vocabulary re-exported from melinoe so the heap's branded containers and
// their consumers share one authoritative token + marker definition.
pub use melinoe::sync::ThreadLocalToken;
pub use melinoe::InvariantLifetime;

/// A wrapper representing a heap block branded with a compile-time unique lifetime.
pub struct BrandedBlock<'brand, T: ?Sized> {
    pub(crate) ptr: NonNull<T>,
    pub(crate) _marker: InvariantLifetime<'brand>,
}

impl<'brand, T: ?Sized> BrandedBlock<'brand, T> {
    /// Returns the raw pointer to the block's managed memory.
    #[inline(always)]
    pub fn as_ptr(&self) -> *mut T {
        self.ptr.as_ptr()
    }
}

impl<'brand, T> BrandedBlock<'brand, T> {
    /// Casts this branded block to managed memory of a different type,
    /// preserving the brand.
    ///
    /// # Safety
    ///
    /// The returned `BrandedBlock<'brand, U>` is trusted by safe APIs that
    /// interpret the pointee as a `U`: [`crate::Heap::free`] runs
    /// `core::ptr::drop_in_place::<U>` on it and derives its deallocation
    /// path from `size_of_val` of the `U`, [`crate::Heap::realloc`] reads
    /// the pointee's layout the same way, and
    /// [`BrandedCell::from_block`] hands out `&U`/`&mut U`. The caller must
    /// therefore guarantee:
    ///
    /// - **Layout**: the block's allocation is at least `size_of::<U>()`
    ///   bytes and aligned to `align_of::<U>()` (e.g. it was allocated for a
    ///   layout that covers `U`), and
    /// - **Initialization/drop discipline**: either the memory holds a valid
    ///   `U` before any path reads or drops it as one, or the block is
    ///   treated as uninitialized `U` storage — written with a valid `U`
    ///   before such a path (as [`crate::Heap::alloc_init`] does), or
    ///   released exclusively through the non-dropping
    ///   [`crate::Heap::free_uninit`].
    ///
    /// Violating either (for example casting an initialized `usize` block to
    /// `String` and freeing it) is a transmute-and-drop and undefined
    /// behavior.
    #[inline(always)]
    pub unsafe fn cast<U>(self) -> BrandedBlock<'brand, U> {
        BrandedBlock {
            ptr: self.ptr.cast(),
            _marker: self._marker,
        }
    }
}

impl<'brand, T: ?Sized> core::fmt::Debug for BrandedBlock<'brand, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("BrandedBlock")
            .field(&self.ptr.as_ptr())
            .finish()
    }
}

impl<'brand, T: ?Sized> core::fmt::Pointer for BrandedBlock<'brand, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Pointer::fmt(&self.ptr.as_ptr(), f)
    }
}

impl<'brand, T: ?Sized> PartialEq for BrandedBlock<'brand, T> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.ptr.as_ptr(), other.ptr.as_ptr())
    }
}
impl<'brand, T: ?Sized> Eq for BrandedBlock<'brand, T> {}

impl<'brand, T: ?Sized> PartialOrd for BrandedBlock<'brand, T> {
    #[inline(always)]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<'brand, T: ?Sized> Ord for BrandedBlock<'brand, T> {
    #[inline(always)]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.ptr
            .as_ptr()
            .cast::<()>()
            .cmp(&other.ptr.as_ptr().cast::<()>())
    }
}
impl<'brand, T: ?Sized> core::hash::Hash for BrandedBlock<'brand, T> {
    #[inline(always)]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.ptr.hash(state);
    }
}

/// A GhostCell-style shared container allowing interior mutability.
///
/// Permits shared read access and exclusive write access mediated by the
/// melinoe [`ThreadLocalToken`].
///
/// # Variance
///
/// `BrandedCell<'brand, T>` is **invariant in `T`** (and in `'brand`). This
/// is a soundness requirement, not a convenience: the cell is `Copy` and
/// writable through [`borrow_mut`](Self::borrow_mut), so a covariant cell
/// would allow a safe lifetime-shortening coercion of one copy (e.g.
/// `BrandedCell<'brand, &'static str>` → `BrandedCell<'brand, &'a str>`),
/// a write of a short-lived `&'a str` through the coerced copy, and a read
/// of the *original* copy as `&'static str` — a dangling reference with no
/// `unsafe` at the call site. This is exactly why [`GhostCell`] wraps its
/// payload in the invariant [`core::cell::UnsafeCell`]; here the payload
/// lives behind a (covariant) [`NonNull`], so invariance is pinned
/// explicitly by the `PhantomData<*mut T>` field.
///
/// [`GhostCell`]: https://plv.mpi-sws.org/rustbelt/ghostcell/
///
/// # Examples
///
/// Token-mediated shared reads and exclusive writes across `Copy` handles:
///
/// ```
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper;
/// use mnemosyne_heap::{scope, BrandedCell};
///
/// scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
///     let block = heap.alloc_init(&token, 41).expect("cell allocation failed");
///     // SAFETY: `alloc_init` returned a block holding an initialized value.
///     let cell = unsafe { BrandedCell::from_block(block) };
///     let copy = cell; // `Copy`: multiple shared handles to one value
///     *cell.borrow_mut(&mut token) += 1;
///     assert_eq!(*copy.borrow(&token), 42);
///     // SAFETY: `cell`/`copy` are the only handles and neither is used again.
///     heap.free(&mut token, unsafe { cell.into_block() });
/// });
/// ```
///
/// The covariant coercion described above fails to compile — the invariance
/// marker rejects shortening the lifetime inside `T`:
///
/// ```compile_fail
/// use mnemosyne_heap::BrandedCell;
///
/// fn shorten<'brand, 'a>(
///     cell: BrandedCell<'brand, &'static str>,
/// ) -> BrandedCell<'brand, &'a str> {
///     cell // ERROR: `BrandedCell` is invariant in `T`
/// }
/// ```
pub struct BrandedCell<'brand, T: ?Sized> {
    pub(crate) ptr: NonNull<T>,
    pub(crate) _marker: InvariantLifetime<'brand>,
    /// Pins the cell invariant in `T`. `NonNull<T>` alone is covariant in
    /// `T`; `*mut T` is invariant in `T` and valid for `T: ?Sized`. The
    /// auto-trait posture is unchanged: `NonNull<T>` already suppresses
    /// `Send`/`Sync`, and `PhantomData<*mut T>` is `Copy`.
    _invariance: PhantomData<*mut T>,
}

impl<'brand, T: ?Sized> Clone for BrandedCell<'brand, T> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<'brand, T: ?Sized> Copy for BrandedCell<'brand, T> {}

impl<'brand, T: ?Sized> BrandedCell<'brand, T> {
    /// Creates a new `BrandedCell` from a `BrandedBlock`.
    ///
    /// # Safety
    /// The block must be initialized.
    #[inline(always)]
    pub unsafe fn from_block(block: BrandedBlock<'brand, T>) -> Self {
        Self {
            ptr: block.ptr,
            _marker: block._marker,
            _invariance: PhantomData,
        }
    }

    /// Returns the raw pointer to the cell's managed memory.
    #[inline(always)]
    pub fn as_ptr(&self) -> *mut T {
        self.ptr.as_ptr()
    }

    /// Consumes the `BrandedCell` (by copy) and reconstructs the `BrandedBlock`.
    ///
    /// # Safety
    /// The caller must ensure that this is the only active reference to the cell,
    /// and that no other copies of this `BrandedCell` will be used to access the memory.
    #[inline(always)]
    pub unsafe fn into_block(self) -> BrandedBlock<'brand, T> {
        BrandedBlock {
            ptr: self.ptr,
            _marker: self._marker,
        }
    }

    /// Accesses the value immutably using the allocator token.
    #[inline(always)]
    pub fn borrow<'a>(&self, _token: &'a ThreadLocalToken<'brand>) -> &'a T {
        // SAFETY: `self.ptr` addresses a live, initialized `T` owned within this
        // brand. There is exactly one `ThreadLocalToken<'brand>` per `'brand`,
        // so a shared `&'a token` proves no `&mut` to the same value can
        // coexist for `'a`. The returned `&'a T` is bound to the token borrow,
        // so the shared reference cannot outlive that exclusivity guarantee —
        // the GhostCell token-aliasing invariant for shared access.
        unsafe { self.ptr.as_ref() }
    }

    /// Accesses the value mutably using the allocator token.
    #[inline(always)]
    pub fn borrow_mut<'a>(&self, _token: &'a mut ThreadLocalToken<'brand>) -> &'a mut T {
        // SAFETY: `self.ptr` addresses a live, initialized `T` owned within this
        // brand. There is exactly one `ThreadLocalToken<'brand>` per `'brand`,
        // and an exclusive `&'a mut token` borrows that sole token, so for `'a`
        // no other `borrow`/`borrow_mut` against this brand can run and no other
        // reference to this value can coexist. The returned `&'a mut T` is bound
        // to the token's exclusive borrow, upholding the unique-mutable-access
        // half of the GhostCell token-aliasing invariant.
        unsafe { &mut *self.ptr.as_ptr() }
    }

    /// Mutably borrows two distinct cells at the same time.
    ///
    /// # Panics
    /// Panics if the two cells point to the same memory block.
    #[inline]
    pub fn borrow_mut_2<'a, U: ?Sized>(
        cell1: &'a Self,
        cell2: &'a BrandedCell<'brand, U>,
        _token: &'a mut ThreadLocalToken<'brand>,
    ) -> (&'a mut T, &'a mut U) {
        assert_ne!(
            cell1.ptr.as_ptr() as *const (),
            cell2.ptr.as_ptr() as *const (),
            "borrow_mut_2: cells must be distinct"
        );
        // SAFETY: the `assert_ne!` above proves `cell1` and `cell2` address
        // disjoint blocks, so the two `&mut` references never alias. Both cells
        // share `'brand`, and the single exclusive `&'a mut token` proves no
        // other access to this brand runs for `'a`. Each pointer addresses a
        // live, initialized value owned within this brand, so simultaneously
        // forming the two mutable references is sound (token-mediated exclusion
        // plus distinctness gives the non-aliasing guarantee).
        unsafe { (&mut *cell1.ptr.as_ptr(), &mut *cell2.ptr.as_ptr()) }
    }

    /// Mutably borrows three distinct cells at the same time.
    ///
    /// # Panics
    /// Panics if any of the cells point to the same memory block.
    #[inline]
    pub fn borrow_mut_3<'a, U: ?Sized, V: ?Sized>(
        cell1: &'a Self,
        cell2: &'a BrandedCell<'brand, U>,
        cell3: &'a BrandedCell<'brand, V>,
        _token: &'a mut ThreadLocalToken<'brand>,
    ) -> (&'a mut T, &'a mut U, &'a mut V) {
        let p1 = cell1.ptr.as_ptr() as *const ();
        let p2 = cell2.ptr.as_ptr() as *const ();
        let p3 = cell3.ptr.as_ptr() as *const ();
        assert!(
            p1 != p2 && p2 != p3 && p1 != p3,
            "borrow_mut_3: cells must be distinct"
        );
        // SAFETY: the `assert!` above proves `cell1`, `cell2`, `cell3` address
        // pairwise-distinct blocks, so the three `&mut` references never alias.
        // All cells share `'brand`, and the single exclusive `&'a mut token`
        // proves no other access to this brand runs for `'a`. Each pointer
        // addresses a live, initialized value owned within this brand, so
        // simultaneously forming the three mutable references is sound
        // (token-mediated exclusion plus pairwise distinctness).
        unsafe {
            (
                &mut *cell1.ptr.as_ptr(),
                &mut *cell2.ptr.as_ptr(),
                &mut *cell3.ptr.as_ptr(),
            )
        }
    }
}

impl<'brand, T: ?Sized> core::fmt::Debug for BrandedCell<'brand, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("BrandedCell")
            .field(&self.ptr.as_ptr())
            .finish()
    }
}

impl<'brand, T: ?Sized> PartialEq for BrandedCell<'brand, T> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.ptr.as_ptr(), other.ptr.as_ptr())
    }
}
impl<'brand, T: ?Sized> Eq for BrandedCell<'brand, T> {}

impl<'brand, T: ?Sized> core::hash::Hash for BrandedCell<'brand, T> {
    #[inline(always)]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.ptr.hash(state);
    }
}

/// Executes a closure with a fresh, compile-time unique branded heap and token.
///
/// # Examples
///
/// ```
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper;
/// use mnemosyne_heap::scope;
///
/// scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
///     let val = mnemosyne_heap::BrandedBox::new(&heap, &token, 42)
///         .expect("branded box allocation failed");
///     assert_eq!(*val, 42);
/// });
/// ```
///
/// This example fails to compile because it attempts to escape a branded block from its scope:
///
/// ```compile_fail
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper;
/// use mnemosyne_heap::{scope, BrandedBlock};
///
/// let mut escaped: Option<BrandedBlock<'static, i32>> = None;
/// scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
///     let block = heap.alloc_init(&token, 42)
///         .expect("branded block allocation failed");
///     // This compile error is expected because the 'brand lifetime cannot escape the closure scope:
///     escaped = Some(block);
/// });
/// ```
///
/// Proving that thread-exclusivity bounds are enforced at compile time.
/// Since the melinoe `ThreadLocalToken` is `!Send` and `!Sync`, the following fails to compile:
///
/// ```compile_fail
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper;
/// use mnemosyne_heap::scope;
/// use std::thread;
///
/// scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
///     // ThreadLocalToken is !Send, so sending it to another thread is a compile error:
///     thread::spawn(move || {
///         let _t = token;
///     });
/// });
/// ```
///
/// Proving that `BrandedBox` is `!Send`:
///
/// ```compile_fail
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper;
/// use mnemosyne_heap::scope;
/// use std::thread;
///
/// scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
///     let val = heap.alloc_init(&token, 42)
///         .expect("branded box send-bound allocation failed");
///     let boxed = unsafe { mnemosyne_heap::BrandedBox::from_raw(&heap, val) };
///     // BrandedBox is !Send, so sending it to another thread is a compile error:
///     thread::spawn(move || {
///         let _b = boxed;
///     });
/// });
/// ```
///
/// Proving that `BrandedVec` is `!Send`:
///
/// ```compile_fail
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper;
/// use mnemosyne_heap::{scope, BrandedVec};
/// use std::thread;
///
/// scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
///     let mut vec = BrandedVec::new(&heap);
///     // BrandedVec is !Send, so sending it to another thread is a compile error:
///     thread::spawn(move || {
///         let _v = vec;
///     });
/// });
/// ```
///
/// Proving that two distinct scopes cannot mix allocation tokens or heaps:
///
/// ```compile_fail
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper;
/// use mnemosyne_heap::scope;
///
/// scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap1, mut token1| {
///     scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap2, mut token2| {
///         let val = heap1.alloc_init(&token1, 42)
///             .expect("cross-scope branded allocation failed");
///         // This fails to compile because token2 has a different 'brand:
///         heap2.free(&mut token2, val);
///     });
/// });
/// ```
pub fn scope<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>, F, R>(f: F) -> R
where
    F: for<'brand> FnOnce(Heap<'brand, P, B>, ThreadLocalToken<'brand>) -> R,
{
    // The brand identity, uniqueness, and thread-confined capability token are
    // minted by melinoe. The higher-ranked `'brand` from `thread_local_scope`
    // is shared with the `Heap` constructed under it, so the heap and its token
    // are provably the only pair for this brand and cannot escape the closure.
    thread_local_scope(|token| {
        let heap = Heap {
            raw: RawHeap::new(),
            _phantom: PhantomData,
        };
        f(heap, token)
    })
}
