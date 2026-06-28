use crate::brand::{BrandedCell, ThreadLocalToken};
use crate::BrandedVec;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::internal::HasSegmentPool;
use mnemosyne_local::LocalAllocatorSelector;

impl<'brand, 'heap, T, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    BrandedVec<'brand, 'heap, T, P, B>
{
    /// Pushes an element onto the back of the vector, growing it if necessary.
    #[inline]
    pub fn push(&mut self, token: &mut ThreadLocalToken<'brand>, val: T) -> Result<(), T> {
        if core::mem::size_of::<T>() == 0 {
            self.len = match self.len.checked_add(1) {
                Some(len) => len,
                None => return Err(val),
            };
            // SAFETY: `T` is zero-sized, so `self.ptr` (the `NonNull::dangling()`
            // sentinel, always aligned) is valid for a zero-byte write. The write
            // consumes `val` by move and records it logically by the `len`
            // increment above; no storage is read or aliased.
            unsafe {
                self.ptr.as_ptr().write(val);
            }
            return Ok(());
        }

        if self.len == self.cap {
            // Capacity policy lives here (initial 4, then amortized doubling);
            // the alloc/realloc mechanics are the shared `grow_to` SSOT. A failed
            // grow returns the element to the caller unconsumed.
            let new_cap = if self.cap == 0 {
                4
            } else {
                match self.cap.checked_mul(2) {
                    Some(cap) => cap,
                    None => return Err(val),
                }
            };
            if self.grow_to(token, new_cap).is_err() {
                return Err(val);
            }
        }
        // SAFETY: the block above guarantees `self.len < self.cap` for non-ZST
        // `T`, so the slot at offset `self.len` lies within the allocation and is
        // currently uninitialized. `self.ptr.add(self.len)` stays in bounds of
        // the live block and is properly aligned for `T`. `write` moves `val`
        // into that slot without reading or dropping prior contents; the `len`
        // increment then claims it as initialized.
        unsafe {
            self.ptr.as_ptr().add(self.len).write(val);
        }
        self.len += 1;
        Ok(())
    }

    /// Pops the last element from the vector, returning it or None if empty.
    #[inline(always)]
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            // SAFETY: `self.len` was `> 0` and is decremented above, so the new
            // `self.len` indexes the last initialized element (for ZST `T`,
            // `add` is a no-op on the dangling sentinel and `read` reconstructs a
            // value with no storage). The slot is in bounds and holds an
            // initialized `T`; `read` moves it out, and the decremented `len`
            // ensures that slot is never read again (no double-drop).
            unsafe { Some(self.ptr.as_ptr().add(self.len).read()) }
        }
    }

    /// Returns the number of elements in the vector.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns true if the vector contains no elements.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the capacity of the vector.
    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// Extracts a slice containing the entire vector.
    #[inline(always)]
    pub fn as_slice(&self) -> &[T] {
        if self.len == 0 {
            &[]
        } else {
            // SAFETY: `self.len > 0` here, so `self.ptr` addresses a live
            // allocation whose prefix `[0, self.len)` is fully initialized `T`
            // (for ZST `T`, the dangling sentinel is a valid base for a
            // zero-sized slice). The pointer is aligned for `T`. `&self` borrows
            // the vector for the returned slice's lifetime, and `BrandedVec` is
            // `!Send`/`!Sync`, so no concurrent mutation can occur.
            unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
        }
    }

    /// Extracts a mutable slice containing the entire vector.
    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        if self.len == 0 {
            &mut []
        } else {
            // SAFETY: same validity argument as `as_slice` — `[0, self.len)` is
            // initialized `T` over a live, aligned allocation. `&mut self` proves
            // exclusive access for the returned slice's lifetime, so the unique
            // borrow required by `from_raw_parts_mut` holds.
            unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
        }
    }

    /// Clears the vector, removing all values.
    ///
    /// Note that this method has no effect on the allocated capacity of the vector.
    #[inline]
    pub fn clear(&mut self) {
        self.truncate(0);
    }

    /// Shortens the vector, keeping the first `len` elements and dropping the rest.
    ///
    /// If `len` is greater than the vector's current length, this has no effect.
    #[inline]
    pub fn truncate(&mut self, len: usize) {
        if len < self.len {
            // SAFETY: `len < self.len`, so the tail range `[len, self.len)` lies
            // within the initialized prefix of the live allocation; `add(len)`
            // stays in bounds and aligned, and `remaining = self.len - len`
            // elements are all initialized `T`. `self.len` is truncated to `len`
            // *before* `drop_in_place`, so the tail is logically removed first: a
            // panic in an element's `Drop` cannot cause the same slots to be
            // dropped again. `&mut self` guarantees exclusive access.
            unsafe {
                let remaining = self.len - len;
                let tail = core::slice::from_raw_parts_mut(self.ptr.as_ptr().add(len), remaining);
                self.len = len;
                core::ptr::drop_in_place(tail);
            }
        }
    }

    /// Clones and appends all elements in a slice to the vector.
    ///
    /// # Errors
    /// Returns `Err(())` if capacity overflow or allocation fails.
    #[inline]
    #[allow(clippy::result_unit_err)] // Preserve the existing allocation-failure API.
    pub fn extend_from_slice(
        &mut self,
        token: &mut ThreadLocalToken<'brand>,
        other: &[T],
    ) -> Result<(), ()>
    where
        T: Clone,
    {
        self.reserve(token, other.len())?;
        for item in other {
            self.push(token, item.clone()).map_err(|_| ())?;
        }
        Ok(())
    }

    /// Resizes the vector in-place so that `len` is equal to `new_len`.
    ///
    /// If `new_len` is greater than `len`, the vector is extended by the difference,
    /// with each additional slot filled with a clone of `value`.
    /// If `new_len` is less than `len`, the vector is truncated.
    ///
    /// # Errors
    /// Returns `Err(())` if capacity overflow or allocation fails.
    #[inline]
    #[allow(clippy::result_unit_err)] // Preserve the existing allocation-failure API.
    pub fn resize(
        &mut self,
        token: &mut ThreadLocalToken<'brand>,
        new_len: usize,
        value: T,
    ) -> Result<(), ()>
    where
        T: Clone,
    {
        if new_len > self.len {
            self.reserve(token, new_len - self.len)?;
            while self.len < new_len {
                self.push(token, value.clone()).map_err(|_| ())?;
            }
        } else {
            self.truncate(new_len);
        }
        Ok(())
    }

    /// Extends the vector with the contents of an iterator.
    ///
    /// # Errors
    /// Returns `Err(())` if allocation fails.
    #[inline]
    #[allow(clippy::result_unit_err)] // Preserve the existing allocation-failure API.
    pub fn extend<I>(&mut self, token: &mut ThreadLocalToken<'brand>, iter: I) -> Result<(), ()>
    where
        I: IntoIterator<Item = T>,
    {
        let iterator = iter.into_iter();
        let (lower, _) = iterator.size_hint();
        if lower > 0 {
            self.reserve(token, lower)?;
        }
        for item in iterator {
            self.push(token, item).map_err(|_| ())?;
        }
        Ok(())
    }

    /// Inserts an element at position `index` within the vector, shifting all elements after it to the right.
    ///
    /// # Panics
    /// Panics if `index > len`.
    ///
    /// # Errors
    /// Returns `Err(element)` if growing the vector fails.
    #[inline]
    pub fn insert(
        &mut self,
        token: &mut ThreadLocalToken<'brand>,
        index: usize,
        element: T,
    ) -> Result<(), T> {
        assert!(index <= self.len, "insert index out of bounds");
        if self.len == self.cap && self.reserve(token, 1).is_err() {
            return Err(element);
        }
        // SAFETY: `index <= self.len` (asserted) and the block above ensured
        // `self.len < self.cap`, so `self.len + 1 <= self.cap`: both the source
        // range `[index, self.len)` and the shifted destination
        // `[index + 1, self.len + 1)` lie within the live, aligned allocation.
        // `copy` (overlap-safe) moves the `self.len - index` initialized tail
        // elements up by one, leaving slot `index` logically vacant;
        // `p.write(element)` fills it without dropping (the prior occupant was
        // moved, not overwritten in place), and the `len` increment claims the
        // new element. Each element is bitwise-moved exactly once, so no
        // double-drop or leak occurs.
        unsafe {
            let p = self.ptr.as_ptr().add(index);
            if index < self.len {
                core::ptr::copy(p, p.add(1), self.len - index);
            }
            p.write(element);
            self.len += 1;
        }
        Ok(())
    }

    /// Removes and returns the element at position `index` within the vector, shifting all elements after it to the left.
    ///
    /// # Panics
    /// Panics if `index >= len`.
    #[inline]
    pub fn remove(&mut self, index: usize) -> T {
        assert!(index < self.len, "remove index out of bounds");
        // SAFETY: `index < self.len` (asserted), so slot `index` holds an
        // initialized `T` within the live, aligned allocation. `read` moves it
        // out (the slot becomes logically vacant); `self.len` is then
        // decremented, after which the overlap-safe `copy` shifts the
        // `self.len - index` initialized tail elements at `[index + 1, old_len)`
        // down by one into `[index, new_len)`. Every element is bitwise-moved
        // exactly once and the removed value is returned by move, so no element
        // is dropped twice or leaked.
        unsafe {
            let p = self.ptr.as_ptr().add(index);
            let val = core::ptr::read(p);
            self.len -= 1;
            if index < self.len {
                core::ptr::copy(p.add(1), p, self.len - index);
            }
            val
        }
    }

    /// Converts this vector into a shared `BrandedCell` containing a slice.
    ///
    /// The memory is shrunk to fit and remains allocated until manually reclaimed.
    #[inline(always)]
    pub fn into_cell(self, token: &mut ThreadLocalToken<'brand>) -> BrandedCell<'brand, [T]> {
        self.into_boxed_slice(token).into_cell()
    }
}

impl<'brand, 'heap, T: Clone, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    BrandedVec<'brand, 'heap, T, P, B>
{
    /// Clones the vector using the given allocator token.
    ///
    /// Returns `None` if allocation fails.
    #[inline]
    pub fn clone_in(&self, token: &mut ThreadLocalToken<'brand>) -> Option<Self> {
        let mut new_vec = Self::with_capacity(self.heap, token, self.len())?;
        for item in self.as_slice() {
            if new_vec.push(token, item.clone()).is_err() {
                return None;
            }
        }
        Some(new_vec)
    }
}
