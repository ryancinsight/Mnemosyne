//! GhostCell-style interior mutability for branded heap blocks.

use core::marker::PhantomData;
use core::ptr::NonNull;

use melinoe::{InvariantLifetime, ReadPermit, WritePermit};

use super::BrandedBlock;

/// A GhostCell-style shared container allowing interior mutability.
///
/// Permits shared read access and exclusive write access mediated by a Melinoe
/// [`ReadPermit`] or [`WritePermit`]. Thread-confined heaps use
/// [`super::ThreadLocalToken`]; [`super::SyncRegionToken`] enables an explicit
/// cross-thread handoff for `Send` payloads.
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
    /// raw pointer marker is deliberately non-`Send`/non-`Sync`; the explicit
    /// unsafe impls below restore those auto traits only under the payload
    /// bounds discharged by the permit contract. `PhantomData<*mut T>` is
    /// `Copy`, so the marker does not add storage.
    _invariance: PhantomData<*mut T>,
}

// SAFETY: moving a cell moves only its pointer and invariant brand marker. The
// pointer is dereferenced exclusively through a matching Melinoe permit, so a
// cross-thread move is sound whenever the payload itself is `Send`.
unsafe impl<'brand, T: ?Sized + Send> Send for BrandedCell<'brand, T> {}

// SAFETY: shared access to a cell exposes only `&T` under a read permit; mutable
// access still requires the unique write permit. Concurrent use is therefore
// sound when `T` is both `Send` and `Sync`, matching MelinoeCell's contract.
unsafe impl<'brand, T: ?Sized + Send + Sync> Sync for BrandedCell<'brand, T> {}

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

    /// Accesses the value immutably using a Melinoe read permit.
    #[inline(always)]
    pub fn borrow<'a, P>(&'a self, _permit: P) -> &'a T
    where
        P: ReadPermit<'brand> + 'a,
    {
        // SAFETY: `self.ptr` addresses a live, initialized `T` owned within this
        // brand. A `ReadPermit<'brand>` proves no exclusive permit for the same
        // brand can coexist for `'a`. The returned `&'a T` is bound to the
        // permit's borrow, so the shared reference cannot outlive that
        // exclusivity guarantee — the GhostCell token-aliasing invariant for
        // shared access.
        unsafe { self.ptr.as_ref() }
    }

    /// Accesses the value mutably using a Melinoe write permit.
    #[inline(always)]
    pub fn borrow_mut<'a, P>(&self, _permit: &'a mut P) -> &'a mut T
    where
        for<'permit> &'permit mut P: WritePermit<'brand>,
    {
        // SAFETY: `self.ptr` addresses a live, initialized `T` owned within this
        // brand. A `WritePermit<'brand>` proves exclusive access for `'a`, so no
        // other `borrow`/`borrow_mut` against this brand can run and no other
        // reference to this value can coexist. The returned `&'a mut T` is bound
        // to the exclusive permit, upholding the unique-mutable-access half of
        // the GhostCell token-aliasing invariant.
        unsafe { &mut *self.ptr.as_ptr() }
    }

    /// Mutably borrows two distinct cells at the same time.
    ///
    /// # Panics
    /// Panics if the two cells point to the same memory block.
    #[inline]
    pub fn borrow_mut_2<'a, U: ?Sized, P>(
        cell1: &'a Self,
        cell2: &'a BrandedCell<'brand, U>,
        _permit: &'a mut P,
    ) -> (&'a mut T, &'a mut U)
    where
        for<'permit> &'permit mut P: WritePermit<'brand>,
    {
        assert_ne!(
            cell1.ptr.as_ptr() as *const (),
            cell2.ptr.as_ptr() as *const (),
            "borrow_mut_2: cells must be distinct"
        );
        // SAFETY: the `assert_ne!` above proves `cell1` and `cell2` address
        // disjoint blocks, so the two `&mut` references never alias. Both cells
        // share `'brand`, and the exclusive write permit proves no other access
        // to this brand runs for `'a`. Each pointer addresses a live,
        // initialized value owned within this brand, so simultaneously forming
        // the two mutable references is sound (permit-mediated exclusion plus
        // distinctness gives the non-aliasing guarantee).
        unsafe { (&mut *cell1.ptr.as_ptr(), &mut *cell2.ptr.as_ptr()) }
    }

    /// Mutably borrows three distinct cells at the same time.
    ///
    /// # Panics
    /// Panics if any of the cells point to the same memory block.
    #[inline]
    pub fn borrow_mut_3<'a, U: ?Sized, V: ?Sized, P>(
        cell1: &'a Self,
        cell2: &'a BrandedCell<'brand, U>,
        cell3: &'a BrandedCell<'brand, V>,
        _permit: &'a mut P,
    ) -> (&'a mut T, &'a mut U, &'a mut V)
    where
        for<'permit> &'permit mut P: WritePermit<'brand>,
    {
        let p1 = cell1.ptr.as_ptr() as *const ();
        let p2 = cell2.ptr.as_ptr() as *const ();
        let p3 = cell3.ptr.as_ptr() as *const ();
        assert!(
            p1 != p2 && p2 != p3 && p1 != p3,
            "borrow_mut_3: cells must be distinct"
        );
        // SAFETY: the `assert!` above proves `cell1`, `cell2`, `cell3` address
        // pairwise-distinct blocks, so the three `&mut` references never alias.
        // All cells share `'brand`, and the exclusive write permit proves no
        // other access to this brand runs for `'a`. Each pointer addresses a
        // live, initialized value owned within this brand, so simultaneously
        // forming the three mutable references is sound (permit-mediated
        // exclusion plus pairwise distinctness).
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
