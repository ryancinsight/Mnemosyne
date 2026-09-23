//! Element-level mutation operations for `AlignedVec`.
//!
//! Contains removal, insertion, filtering, partitioning, deduplication,
//! bulk-transfer, capacity-shrinking, and draining operations. These differ
//! from the length-and-grow operations in `super::length` in that they
//! remove or restructure individual elements rather than uniformly growing or
//! filling the buffer.
//!
//! The `drain` method here returns a `super::length::Drain` iterator
//! defined in the sibling module.

use super::super::element::ScratchElement;
use super::length::Drain;
use super::storage::AlignedVec;

impl<T: ScratchElement> AlignedVec<T> {
    // ── Removal ──────────────────────────────────────────────────────────────

    /// Removes and returns the last element, or `None` if empty. O(1).
    #[inline]
    #[must_use]
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        // SAFETY: `self.len` was decremented, so the element at the former last
        // position is still inside the allocation and was initialized; `T: Copy`
        // makes a bitwise read safe.
        Some(unsafe { core::ptr::read(self.ptr.add(self.len)) })
    }

    /// Removes and returns the element at `index` by swapping it with the last.
    ///
    /// Does **not** preserve element order. O(1).
    ///
    /// # Panics
    ///
    /// Panics if `index >= self.len()`.
    #[inline]
    #[must_use]
    pub fn swap_remove(&mut self, index: usize) -> T {
        assert!(
            index < self.len,
            "swap_remove: index {index} >= len {}",
            self.len
        );
        // SAFETY: both `index` and `self.len - 1` are `< self.len`, so both are
        // inside the initialized region; `T: Copy`.
        let last = unsafe { core::ptr::read(self.ptr.add(self.len - 1)) };
        let removed = unsafe { core::ptr::read(self.ptr.add(index)) };
        if index != self.len - 1 {
            // SAFETY: writing to `index < self.len` stays inside the allocation.
            unsafe { core::ptr::write(self.ptr.add(index), last) };
        }
        self.len -= 1;
        removed
    }

    /// Removes and returns the element at `index`, shifting later elements left.
    ///
    /// Preserves order. O(n).
    ///
    /// # Panics
    ///
    /// Panics if `index >= self.len()`.
    #[inline]
    #[must_use]
    pub fn remove(&mut self, index: usize) -> T {
        assert!(
            index < self.len,
            "remove: index {index} >= len {}",
            self.len
        );
        // SAFETY: `index < len` so the element is initialized; `T: Copy`.
        let removed = unsafe { core::ptr::read(self.ptr.add(index)) };
        let tail = self.len - index - 1;
        if tail > 0 {
            // SAFETY: source `[index+1, len)` and destination `[index, len-1)` may
            // overlap, so we use `copy` (memmove semantics), not
            // `copy_nonoverlapping`.
            unsafe {
                core::ptr::copy(self.ptr.add(index + 1), self.ptr.add(index), tail);
            }
        }
        self.len -= 1;
        removed
    }

    // ── Insertion ────────────────────────────────────────────────────────────

    /// Inserts `value` at `index`, shifting later elements right. O(n).
    ///
    /// # Panics
    ///
    /// Panics if `index > self.len()`.
    #[inline]
    pub fn insert(&mut self, index: usize, value: T) {
        assert!(
            index <= self.len,
            "insert: index {index} > len {}",
            self.len
        );
        self.reserve(1);
        if index < self.len {
            // SAFETY: `index < len < capacity` (reserve ensured it). Source
            // `[index, len)` and destination `[index+1, len+1)` may overlap, so
            // we use `copy` (memmove).
            unsafe {
                core::ptr::copy(
                    self.ptr.add(index),
                    self.ptr.add(index + 1),
                    self.len - index,
                );
            }
        }
        // SAFETY: `ptr.add(index)` is inside the now-larger allocation.
        unsafe { core::ptr::write(self.ptr.add(index), value) };
        self.len += 1;
    }

    // ── Filtering ────────────────────────────────────────────────────────────

    /// Retains only elements satisfying `predicate`, removing others in-place.
    ///
    /// Preserves relative order. No reallocation.
    #[inline]
    pub fn retain<F: FnMut(&T) -> bool>(&mut self, mut predicate: F) {
        let mut write = 0usize;
        for read in 0..self.len {
            // SAFETY: `read < self.len` — inside the initialized region; T: Copy.
            let elem = unsafe { core::ptr::read(self.ptr.add(read)) };
            if predicate(&elem) {
                if write != read {
                    // SAFETY: `write <= read < self.len`; T: Copy.
                    unsafe { core::ptr::write(self.ptr.add(write), elem) };
                }
                write += 1;
            }
        }
        self.len = write;
    }

    // ── Partitioning ─────────────────────────────────────────────────────────

    /// Partitions the buffer in-place around a predicate.
    ///
    /// All `true` elements come before all `false` elements. Order within each
    /// group is not preserved. Returns the count of `true` elements (the pivot
    /// index). O(n), no allocation.
    #[inline]
    pub fn partition_in_place<F: FnMut(&T) -> bool>(&mut self, mut predicate: F) -> usize {
        let mut lo = 0usize;
        let mut hi = self.len;
        loop {
            while lo < hi {
                // SAFETY: `lo < hi <= self.len`.
                let elem = unsafe { core::ptr::read(self.ptr.add(lo)) };
                if predicate(&elem) {
                    lo += 1;
                } else {
                    break;
                }
            }
            while lo < hi {
                hi -= 1;
                // SAFETY: `hi < self.len`.
                let elem = unsafe { core::ptr::read(self.ptr.add(hi)) };
                if predicate(&elem) {
                    break;
                }
            }
            if lo >= hi {
                break;
            }
            // SAFETY: lo and hi are distinct valid indices.
            unsafe {
                let a = core::ptr::read(self.ptr.add(lo));
                let b = core::ptr::read(self.ptr.add(hi));
                core::ptr::write(self.ptr.add(lo), b);
                core::ptr::write(self.ptr.add(hi), a);
            }
            lo += 1;
        }
        lo
    }

    // ── Deduplication ────────────────────────────────────────────────────────

    /// Removes consecutive duplicate elements.
    ///
    /// Sort first to deduplicate globally. In-place, no allocation.
    #[inline]
    pub fn dedup(&mut self)
    where
        T: PartialEq,
    {
        self.dedup_by_key(|x| *x);
    }

    /// Removes consecutive duplicates according to a key function.
    ///
    /// Two adjacent elements `a` and `b` are considered duplicates when
    /// `key(a) == key(b)`. The first of each run is kept.
    #[inline]
    pub fn dedup_by_key<K: PartialEq, F: FnMut(&T) -> K>(&mut self, mut key: F) {
        if self.len < 2 {
            return;
        }
        let mut write = 1usize;
        for read in 1..self.len {
            // SAFETY: `read` and `write - 1` are both `< self.len`; T: Copy.
            let elem = unsafe { core::ptr::read(self.ptr.add(read)) };
            let prev = unsafe { core::ptr::read(self.ptr.add(write - 1)) };
            if key(&elem) != key(&prev) {
                if write != read {
                    unsafe { core::ptr::write(self.ptr.add(write), elem) };
                }
                write += 1;
            }
        }
        self.len = write;
    }

    // ── Bulk transfer ────────────────────────────────────────────────────────

    /// Moves all elements of `other` into `self`, leaving `other` empty.
    #[inline]
    pub fn append(&mut self, other: &mut Self) {
        if !other.is_empty() {
            self.extend_from_slice(other.as_slice());
            other.len = 0;
        }
    }

    // ── Splitting ────────────────────────────────────────────────────────────

    /// Splits off `[at, len)` into a new `AlignedVec`; `self` keeps `[0, at)`.
    ///
    /// # Panics
    ///
    /// Panics if `at > self.len()`.
    #[must_use]
    #[inline]
    pub fn split_off(&mut self, at: usize) -> Self {
        assert!(at <= self.len, "split_off: at {at} > len {}", self.len);
        let tail_len = self.len - at;
        // SAFETY: `[at, at + tail_len)` is within the initialized region; T: Copy.
        let tail =
            Self::from_slice(unsafe { core::slice::from_raw_parts(self.ptr.add(at), tail_len) });
        self.len = at;
        tail
    }

    // ── Capacity management ──────────────────────────────────────────────────

    /// Shrinks the capacity to `len()` if possible.
    ///
    /// Best-effort: on allocator refusal the buffer remains valid unchanged.
    #[inline]
    pub fn shrink_to_fit(&mut self) {
        self.shrink_to(self.len);
    }

    // ── Uninitialised access ─────────────────────────────────────────────────

    /// Returns a raw pointer to the uninitialized spare capacity `[len, capacity)`.
    ///
    /// The caller must initialize every element in the returned slice before
    /// calling `set_len_unchecked` to extend the initialized prefix.
    #[inline]
    pub fn spare_capacity_mut(&mut self) -> *mut [T] {
        let spare_len = self.capacity - self.len;
        // SAFETY: `self.ptr.add(self.len)` is the first byte past the
        // initialized region, inside the allocation (`capacity >= len`).
        unsafe { core::ptr::slice_from_raw_parts_mut(self.ptr.add(self.len), spare_len) }
    }

    // ── Construction helpers ─────────────────────────────────────────────────

    /// Creates a buffer of `len` elements where element `i` is produced by
    /// `f(i)`. Pre-allocates upfront; no intermediate iterator.
    #[must_use]
    #[inline]
    pub fn from_fn<F: FnMut(usize) -> T>(len: usize, mut f: F) -> Self {
        let mut buf = Self::with_capacity(len);
        for i in 0..len {
            buf.push(f(i));
        }
        buf
    }

    /// Overwrites all `len()` initialized elements with `value`.
    #[inline]
    pub fn fill(&mut self, value: T) {
        // SAFETY: `[0, self.len)` is initialized; T: Copy overwrites safely.
        unsafe {
            for i in 0..self.len {
                core::ptr::write(self.ptr.add(i), value);
            }
        }
    }

    /// Resets all initialized elements to the all-zero bit pattern.
    ///
    /// Equivalent to `fill` with the zero value but uses a single
    /// `write_bytes(0)` call — faster than iterating when the size is large.
    ///
    /// Requires the all-zero bit pattern to be a valid value of `T`, which
    /// is guaranteed by the [`ScratchElement`] invariant.
    #[inline]
    pub fn zero_fill(&mut self) {
        if self.len == 0 {
            return;
        }
        // SAFETY: `[0, self.len)` is within the allocation; all-zero is a
        // valid bit pattern for every `ScratchElement` type by invariant.
        unsafe {
            core::ptr::write_bytes(self.ptr, 0, self.len);
        }
    }

    /// Copies a slice of exactly `len()` elements into the buffer.
    ///
    /// Panics if `src.len() != self.len()`.  Equivalent to
    /// `self.as_mut_slice().copy_from_slice(src)` but named for discoverability.
    #[inline]
    pub fn copy_from_slice(&mut self, src: &[T]) {
        self.as_mut_slice().copy_from_slice(src);
    }

    // ── Drain ────────────────────────────────────────────────────────────────

    /// Removes elements in `start..end`, yields them by value, then shifts
    /// later elements left to fill the gap.
    ///
    /// If the iterator is dropped before being fully consumed, remaining
    /// elements in the range are still removed.
    ///
    /// # Panics
    ///
    /// Panics if `start > end` or `end > self.len()`.
    #[inline]
    #[must_use]
    pub fn drain(&mut self, start: usize, end: usize) -> Drain<'_, T> {
        assert!(start <= end, "drain: start > end");
        assert!(end <= self.len, "drain: end > len");
        Drain {
            buf: self,
            start,
            end,
            current: start,
        }
    }

    // ── Bulk operations ──────────────────────────────────────────────────────

    /// Concatenates two slices into a new `AlignedVec`, copying both.
    ///
    /// Equivalent to `AlignedVec::from_slice(a)` + `extend_from_slice(b)`.
    #[inline]
    #[must_use]
    pub fn concat(a: &[T], b: &[T]) -> Self {
        let mut v = Self::with_capacity(a.len() + b.len());
        v.extend_from_slice(a);
        v.extend_from_slice(b);
        v
    }
}
