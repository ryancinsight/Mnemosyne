use core::ptr::NonNull;
use core::alloc::Layout;
use core::ops::{Deref, DerefMut};
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::internal::HasSegmentPool;
use mnemosyne_local::LocalAllocatorSelector;
use crate::brand::{Invariant, AllocatorToken, BrandedBlock, BrandedCell};
use crate::branded_box::BrandedBox;
use crate::BrandedHeap;

/// A dynamically growing array allocated from a `BrandedHeap`.
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
    pub(crate) heap: &'heap BrandedHeap<'brand, P, B>,
    pub(crate) _non_send_sync: core::marker::PhantomData<*mut ()>,
}

impl<'brand, 'heap, T, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    BrandedVec<'brand, 'heap, T, P, B>
{
    /// Creates a new empty `BrandedVec` backed by the given `BrandedHeap`.
    #[inline(always)]
    pub fn new(heap: &'heap BrandedHeap<'brand, P, B>) -> Self {
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
        heap: &'heap BrandedHeap<'brand, P, B>,
        token: &AllocatorToken<'brand>,
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

    /// Pushes an element onto the back of the vector, growing it if necessary.
    #[inline]
    pub fn push(&mut self, token: &mut AllocatorToken<'brand>, val: T) -> Result<(), T> {
        if core::mem::size_of::<T>() == 0 {
            self.len = match self.len.checked_add(1) {
                Some(len) => len,
                None => return Err(val),
            };
            unsafe {
                self.ptr.as_ptr().write(val);
            }
            return Ok(());
        }

        if self.len == self.cap {
            let new_cap = if self.cap == 0 {
                4
            } else {
                match self.cap.checked_mul(2) {
                    Some(cap) => cap,
                    None => return Err(val),
                }
            };
            let new_layout = match Layout::array::<T>(new_cap) {
                Ok(l) => l,
                Err(_) => return Err(val),
            };
            if self.cap == 0 {
                let block = match self.heap.alloc(token, new_layout) {
                    Some(b) => b,
                    None => return Err(val),
                };
                self.ptr = block.ptr.cast();
                self.cap = new_cap;
            } else {
                let old_layout = Layout::array::<T>(self.cap).unwrap_or_else(|_| {
                    debug_assert!(false, "Layout array calculation failed for valid capacity");
                    unsafe { core::hint::unreachable_unchecked() }
                });
                let block = BrandedBlock {
                    ptr: self.ptr,
                    _marker: Invariant::new(),
                };
                let new_block = match self
                    .heap
                    .realloc(token, block, old_layout, new_layout.size())
                {
                    Some(b) => b,
                    None => return Err(val),
                };
                self.ptr = new_block.ptr.cast();
                self.cap = new_cap;
            }
        }
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
            unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
        }
    }

    /// Extracts a mutable slice containing the entire vector.
    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        if self.len == 0 {
            &mut []
        } else {
            unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
        }
    }

    /// Converts this vector into a boxed slice, shrinking the memory allocation to fit.
    #[inline]
    pub fn into_boxed_slice(
        mut self,
        token: &mut AllocatorToken<'brand>,
    ) -> BrandedBox<'brand, 'heap, [T], P, B> {
        if core::mem::size_of::<T>() == 0 {
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

        if self.cap > self.len {
            if self.len == 0 {
                unsafe {
                    self.heap.free_raw(self.ptr.as_ptr() as *mut u8);
                }
                self.ptr = NonNull::dangling();
                self.cap = 0;
            } else {
                let old_layout = Layout::array::<T>(self.cap).unwrap_or_else(|_| {
                    debug_assert!(false, "Layout array calculation failed for valid capacity");
                    unsafe { core::hint::unreachable_unchecked() }
                });
                let block = BrandedBlock {
                    ptr: self.ptr,
                    _marker: Invariant::new(),
                };
                let new_size = core::mem::size_of::<T>() * self.len;
                if let Some(new_block) = self.heap.realloc(token, block, old_layout, new_size) {
                    self.ptr = new_block.ptr.cast();
                    self.cap = self.len;
                }
            }
        }

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
            unsafe {
                let remaining = self.len - len;
                let tail = core::slice::from_raw_parts_mut(self.ptr.as_ptr().add(len), remaining);
                self.len = len;
                core::ptr::drop_in_place(tail);
            }
        }
    }

    /// Reserves capacity for at least `additional` more elements to be inserted in the vector.
    ///
    /// # Errors
    /// Returns `Err(())` if layout calculations overflow or allocation fails.
    #[inline]
    #[allow(clippy::result_unit_err)]
    pub fn reserve(
        &mut self,
        token: &mut AllocatorToken<'brand>,
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
        let new_layout = Layout::array::<T>(new_cap).map_err(|_| ())?;
        if self.cap == 0 {
            let block = self.heap.alloc(token, new_layout).ok_or(())?;
            self.ptr = block.ptr.cast();
            self.cap = new_cap;
        } else {
            let old_layout = Layout::array::<T>(self.cap).unwrap_or_else(|_| {
                debug_assert!(false, "Layout array calculation failed for valid capacity");
                unsafe { core::hint::unreachable_unchecked() }
            });
            let block = BrandedBlock {
                ptr: self.ptr,
                _marker: Invariant::new(),
            };
            let new_block = self
                .heap
                .realloc(token, block, old_layout, new_layout.size())
                .ok_or(())?;
            self.ptr = new_block.ptr.cast();
            self.cap = new_cap;
        }
        Ok(())
    }

    /// Shrinks the capacity of the vector as much as possible.
    ///
    /// # Errors
    /// Returns `Err(())` if allocation fails.
    #[inline]
    #[allow(clippy::result_unit_err)]
    pub fn shrink_to_fit(&mut self, token: &mut AllocatorToken<'brand>) -> Result<(), ()> {
        if core::mem::size_of::<T>() == 0 || self.cap <= self.len {
            return Ok(());
        }
        if self.len == 0 {
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
            _marker: Invariant::new(),
        };
        let new_size = core::mem::size_of::<T>() * self.len;
        if let Some(new_block) = self.heap.realloc(token, block, old_layout, new_size) {
            self.ptr = new_block.ptr.cast();
            self.cap = self.len;
            Ok(())
        } else {
            Err(())
        }
    }

    /// Clones and appends all elements in a slice to the vector.
    ///
    /// # Errors
    /// Returns `Err(())` if capacity overflow or allocation fails.
    #[inline]
    pub fn extend_from_slice(
        &mut self,
        token: &mut AllocatorToken<'brand>,
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
    pub fn resize(
        &mut self,
        token: &mut AllocatorToken<'brand>,
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
    pub fn extend<I>(&mut self, token: &mut AllocatorToken<'brand>, iter: I) -> Result<(), ()>
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
        token: &mut AllocatorToken<'brand>,
        index: usize,
        element: T,
    ) -> Result<(), T> {
        assert!(index <= self.len, "insert index out of bounds");
        if self.len == self.cap && self.reserve(token, 1).is_err() {
            return Err(element);
        }
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
    pub fn into_cell(self, token: &mut AllocatorToken<'brand>) -> BrandedCell<'brand, [T]> {
        self.into_boxed_slice(token).into_cell()
    }
}

impl<'brand, 'heap, T, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> Deref
    for BrandedVec<'brand, 'heap, T, P, B>
{
    type Target = [T];
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<'brand, 'heap, T, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> DerefMut
    for BrandedVec<'brand, 'heap, T, P, B>
{
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<'brand, 'heap, T, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> Drop
    for BrandedVec<'brand, 'heap, T, P, B>
{
    #[inline]
    fn drop(&mut self) {
        if self.cap > 0 || (core::mem::size_of::<T>() == 0 && self.len > 0) {
            unsafe {
                core::ptr::drop_in_place(self.as_mut_slice());
                if core::mem::size_of::<T>() != 0 {
                    self.heap.free_raw(self.ptr.as_ptr() as *mut u8);
                }
            }
        }
    }
}

impl<'brand, 'heap, T: Clone, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    BrandedVec<'brand, 'heap, T, P, B>
{
    /// Clones the vector using the given allocator token.
    ///
    /// Returns `None` if allocation fails.
    #[inline]
    pub fn clone_in(&self, token: &mut AllocatorToken<'brand>) -> Option<Self> {
        let mut new_vec = Self::with_capacity(self.heap, token, self.len())?;
        for item in self.as_slice() {
            if new_vec.push(token, item.clone()).is_err() {
                return None;
            }
        }
        Some(new_vec)
    }
}

impl<
        'brand,
        'heap,
        T: core::fmt::Debug,
        P: AllocPolicy,
        B: HasSegmentPool + LocalAllocatorSelector<B>,
    > core::fmt::Debug for BrandedVec<'brand, 'heap, T, P, B>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self.as_slice(), f)
    }
}

impl<
        'brand,
        'heap,
        T: PartialEq,
        P: AllocPolicy,
        B: HasSegmentPool + LocalAllocatorSelector<B>,
    > PartialEq for BrandedVec<'brand, 'heap, T, P, B>
{
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl<'brand, 'heap, T: Eq, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> Eq
    for BrandedVec<'brand, 'heap, T, P, B>
{
}

impl<
        'brand,
        'heap,
        T: PartialOrd,
        P: AllocPolicy,
        B: HasSegmentPool + LocalAllocatorSelector<B>,
    > PartialOrd for BrandedVec<'brand, 'heap, T, P, B>
{
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.as_slice().partial_cmp(other.as_slice())
    }
}
impl<'brand, 'heap, T: Ord, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> Ord
    for BrandedVec<'brand, 'heap, T, P, B>
{
    #[inline]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}
impl<
        'brand,
        'heap,
        T: core::hash::Hash,
        P: AllocPolicy,
        B: HasSegmentPool + LocalAllocatorSelector<B>,
    > core::hash::Hash for BrandedVec<'brand, 'heap, T, P, B>
{
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}
