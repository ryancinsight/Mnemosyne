//! Stack-resident, fixed-capacity scratch buffer.
//!
//! [`AlignedBuf`] is the zero-heap complement to
//! [`super::aligned_vec::AlignedVec`]: it stores up to `N` elements inline in
//! the struct itself, making it a zero-cost abstraction for small hot-path
//! buffers whose maximum size is known at compile time.
//!
//! # Design
//!
//! The backing store is `[MaybeUninit<T>; N]`. Only the first `len` slots are
//! initialized; unwritten slots hold unspecified bytes. [`ScratchElement`]'s
//! `Copy` super-bound means there are no destructors to run, so `clear` is a
//! single field write and `pop` / `drop` never leak resources.
//!
//! # Zero heap
//!
//! No allocator call is ever made by `AlignedBuf`'s own methods. When
//! monomorphized the optimizer can keep small instances entirely in registers
//! or a stack frame.
//!
//! # Copy
//!
//! `AlignedBuf<T, N>` is `Copy` (every `ScratchElement` is). The copy
//! includes the `len` field, so the semantics are identical to copying
//! `[T; len]`.

use super::element::ScratchElement;
use core::mem::MaybeUninit;

// ── Type ─────────────────────────────────────────────────────────────────────

/// A stack-resident, fixed-capacity buffer holding up to `N` elements of `T`.
///
/// All storage is inline — no heap allocation is ever performed. Use this
/// instead of [`super::aligned_vec::AlignedVec`] when the upper bound on
/// element count is known at compile time and fits on the stack.
///
/// # Example
///
/// ```rust
/// use mnemosyne_arena::AlignedBuf;
///
/// let mut buf = AlignedBuf::<u32, 8>::new();
/// buf.push(1);
/// buf.push(2);
/// assert_eq!(buf.as_slice(), &[1, 2]);
/// assert_eq!(buf.pop(), Some(2));
/// ```
pub struct AlignedBuf<T: ScratchElement, const N: usize> {
    /// Inline storage for up to `N` elements.
    data: [MaybeUninit<T>; N],
    /// Number of initialized elements.
    len: usize,
}

// ── Construction ─────────────────────────────────────────────────────────────

impl<T: ScratchElement, const N: usize> AlignedBuf<T, N> {
    /// Creates an empty buffer with no initialized elements.
    ///
    /// `const fn` — usable in statics and const contexts.
    #[inline]
    pub const fn new() -> Self {
        // SAFETY: `[MaybeUninit<T>; N]` has no validity invariant — every bit
        // pattern is a valid representation of the array type. Calling
        // `assume_init` on the outer `MaybeUninit` wrapper is therefore always
        // sound; it does not create or access any `T` value.
        unsafe {
            Self {
                data: MaybeUninit::uninit().assume_init(),
                len: 0,
            }
        }
    }

    /// Creates a buffer filled with `value` replicated `N` times.
    #[inline]
    #[must_use]
    pub fn filled(value: T) -> Self {
        let mut buf = Self::new();
        for i in 0..N {
            buf.data[i] = MaybeUninit::new(value);
        }
        buf.len = N;
        buf
    }

    /// Creates a buffer from a fixed-size array, consuming all `N` elements.
    #[inline]
    #[must_use]
    pub fn from_array(arr: [T; N]) -> Self {
        let mut buf = Self::new();
        for (i, v) in arr.into_iter().enumerate() {
            buf.data[i] = MaybeUninit::new(v);
        }
        buf.len = N;
        buf
    }

    /// Creates a buffer from a slice, copying up to `N` elements.
    ///
    /// If `slice.len() > N`, only the first `N` elements are copied.
    #[inline]
    #[must_use]
    pub fn from_slice_truncating(slice: &[T]) -> Self {
        let n = slice.len().min(N);
        let mut buf = Self::new();
        for (i, &v) in slice[..n].iter().enumerate() {
            buf.data[i] = MaybeUninit::new(v);
        }
        buf.len = n;
        buf
    }
}

// ── Capacity queries ──────────────────────────────────────────────────────────

impl<T: ScratchElement, const N: usize> AlignedBuf<T, N> {
    /// Maximum number of elements this buffer can hold (always `N`).
    #[inline]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Number of initialized elements currently in the buffer.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if no elements have been pushed.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns `true` when `len == N` and no further push is possible.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len == N
    }

    /// Number of remaining slots before the buffer is full.
    #[inline]
    pub fn remaining(&self) -> usize {
        N - self.len
    }
}

// ── Push / pop ────────────────────────────────────────────────────────────────

impl<T: ScratchElement, const N: usize> AlignedBuf<T, N> {
    /// Appends `value`.
    ///
    /// # Panics
    ///
    /// Panics if the buffer is full (`len == N`).
    #[inline]
    pub fn push(&mut self, value: T) {
        assert!(
            self.len < N,
            "AlignedBuf::push: buffer is full (capacity {N})"
        );
        self.data[self.len] = MaybeUninit::new(value);
        self.len += 1;
    }

    /// Appends `value` without panicking.
    ///
    /// Returns `true` on success, `false` if the buffer is full.
    #[inline]
    pub fn try_push(&mut self, value: T) -> bool {
        if self.len < N {
            self.data[self.len] = MaybeUninit::new(value);
            self.len += 1;
            true
        } else {
            false
        }
    }

    /// Removes and returns the last element, or `None` if empty.
    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        // SAFETY: `self.len` was just decremented, so `data[self.len]` was
        // initialized by a prior `push` or `try_push` call.
        Some(unsafe { self.data[self.len].assume_init_read() })
    }
}

// ── Mutation ─────────────────────────────────────────────────────────────────

impl<T: ScratchElement, const N: usize> AlignedBuf<T, N> {
    /// Resets the length to zero.
    ///
    /// Slots are not cleared; the next push overwrites them. `T: ScratchElement`
    /// (no `Drop`) means no resources leak.
    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Shortens to `new_len`. If `new_len >= len()` this is a no-op.
    #[inline]
    pub fn truncate(&mut self, new_len: usize) {
        if new_len < self.len {
            self.len = new_len;
        }
    }

    /// Zero-fills all initialized elements (sets every byte to `0`).
    ///
    /// All-zero is a valid bit pattern for every [`ScratchElement`] type by
    /// the trait's invariant.
    #[inline]
    pub fn zero_fill(&mut self) {
        if self.len == 0 {
            return;
        }
        // SAFETY: `[0, self.len)` of `self.data` was written by push/try_push,
        // so the pointer and range are valid. All-zero is a valid `T` bit
        // pattern per `ScratchElement`.
        unsafe {
            core::ptr::write_bytes(self.data.as_mut_ptr().cast::<T>(), 0, self.len);
        }
    }
}

// ── Slice views ───────────────────────────────────────────────────────────────

impl<T: ScratchElement, const N: usize> AlignedBuf<T, N> {
    /// Shared slice of the initialized elements.
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: `[0, self.len)` is initialized (every element was written by
        // `push` or `try_push`). `T: ScratchElement` is `Copy` / POD; `&self`
        // ensures exclusive read access for the slice's lifetime.
        unsafe { core::slice::from_raw_parts(self.data.as_ptr().cast::<T>(), self.len) }
    }

    /// Mutable slice of the initialized elements.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: same validity argument as `as_slice`; `&mut self` proves
        // exclusive access.
        unsafe { core::slice::from_raw_parts_mut(self.data.as_mut_ptr().cast::<T>(), self.len) }
    }

    /// Raw pointer to the start of the inline storage.
    ///
    /// Valid for `N` slots, of which the first `len()` are initialized.
    #[inline]
    pub fn as_ptr(&self) -> *const T {
        self.data.as_ptr().cast::<T>()
    }

    /// Mutable raw pointer to the start of the inline storage.
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.data.as_mut_ptr().cast::<T>()
    }
}

// ── Standard trait impls ──────────────────────────────────────────────────────

impl<T: ScratchElement, const N: usize> Default for AlignedBuf<T, N> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ScratchElement + core::fmt::Debug, const N: usize> core::fmt::Debug for AlignedBuf<T, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.as_slice().iter()).finish()
    }
}

impl<T: ScratchElement, const N: usize> Clone for AlignedBuf<T, N> {
    #[inline]
    fn clone(&self) -> Self {
        let mut new = Self::new();
        for &v in self.as_slice() {
            new.data[new.len] = MaybeUninit::new(v);
            new.len += 1;
        }
        new
    }
}

// SAFETY: every `ScratchElement` is `Copy`; `MaybeUninit<T>: Copy` always, so
// the struct copy is a bitwise copy of the inline array + len field.
impl<T: ScratchElement, const N: usize> Copy for AlignedBuf<T, N> {}

impl<T: ScratchElement + PartialEq, const N: usize> PartialEq for AlignedBuf<T, N> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: ScratchElement + Eq, const N: usize> Eq for AlignedBuf<T, N> {}

impl<T: ScratchElement + PartialOrd, const N: usize> PartialOrd for AlignedBuf<T, N> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.as_slice().partial_cmp(other.as_slice())
    }
}

impl<T: ScratchElement + Ord, const N: usize> Ord for AlignedBuf<T, N> {
    #[inline]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}

impl<T: ScratchElement + core::hash::Hash, const N: usize> core::hash::Hash for AlignedBuf<T, N> {
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl<T: ScratchElement, const N: usize> core::ops::Deref for AlignedBuf<T, N> {
    type Target = [T];
    #[inline]
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: ScratchElement, const N: usize> core::ops::DerefMut for AlignedBuf<T, N> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T: ScratchElement, const N: usize> AsRef<[T]> for AlignedBuf<T, N> {
    #[inline]
    fn as_ref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: ScratchElement, const N: usize> AsMut<[T]> for AlignedBuf<T, N> {
    #[inline]
    fn as_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T: ScratchElement, const N: usize> core::borrow::Borrow<[T]> for AlignedBuf<T, N> {
    #[inline]
    fn borrow(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: ScratchElement, const N: usize> core::borrow::BorrowMut<[T]> for AlignedBuf<T, N> {
    #[inline]
    fn borrow_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T: ScratchElement + PartialEq, const N: usize> PartialEq<[T]> for AlignedBuf<T, N> {
    #[inline]
    fn eq(&self, other: &[T]) -> bool {
        self.as_slice() == other
    }
}

// ── Iterators ─────────────────────────────────────────────────────────────────

impl<'a, T: ScratchElement, const N: usize> IntoIterator for &'a AlignedBuf<T, N> {
    type Item = &'a T;
    type IntoIter = core::slice::Iter<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<'a, T: ScratchElement, const N: usize> IntoIterator for &'a mut AlignedBuf<T, N> {
    type Item = &'a mut T;
    type IntoIter = core::slice::IterMut<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.as_mut_slice().iter_mut()
    }
}

impl<T: ScratchElement, const N: usize> core::iter::FromIterator<T> for AlignedBuf<T, N> {
    /// Collects up to `N` elements; any beyond `N` are silently dropped.
    ///
    /// This mirrors [`from_slice_truncating`][AlignedBuf::from_slice_truncating]:
    /// the buffer's fixed capacity cannot grow.
    #[inline]
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut buf = Self::new();
        for item in iter {
            if !buf.try_push(item) {
                break;
            }
        }
        buf
    }
}

// ── Conversions ───────────────────────────────────────────────────────────────

impl<T: ScratchElement, const N: usize> From<[T; N]> for AlignedBuf<T, N> {
    #[inline]
    fn from(arr: [T; N]) -> Self {
        Self::from_array(arr)
    }
}

impl<T: ScratchElement, const N: usize> From<AlignedBuf<T, N>> for [T; N] {
    /// Converts a full buffer into an array.
    ///
    /// # Panics
    ///
    /// Panics if `buf.len() != N`.
    #[inline]
    fn from(buf: AlignedBuf<T, N>) -> [T; N] {
        assert!(
            buf.len == N,
            "AlignedBuf: cannot convert to [T; N], len {} != {N}",
            buf.len
        );
        // SAFETY: `buf.len == N` means all N slots are initialized.
        // `T: ScratchElement: Copy` so reading each by copy is valid.
        core::array::from_fn(|i| unsafe { buf.data[i].assume_init_read() })
    }
}

// ── Send + Sync ───────────────────────────────────────────────────────────────

// SAFETY: no raw pointers; `data` is an inline array of `MaybeUninit<T>`.
// Send/Sync follow directly from `T`'s bounds, which the compiler can derive
// automatically since all fields satisfy the bounds.
// The explicit impls below are provided only to document the reasoning.
//
// Actually the compiler will auto-derive Send/Sync since MaybeUninit<T>: Send
// when T: Send (and same for Sync), and usize is always Send+Sync.
// No manual unsafe impl needed.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_pop() {
        let mut buf = AlignedBuf::<u32, 4>::new();
        assert!(buf.is_empty());
        assert_eq!(buf.capacity(), 4);
        buf.push(10);
        buf.push(20);
        assert_eq!(buf.len(), 2);
        assert!(!buf.is_full());
        assert_eq!(buf.pop(), Some(20));
        assert_eq!(buf.pop(), Some(10));
        assert_eq!(buf.pop(), None);
    }

    #[test]
    fn try_push_full() {
        let mut buf = AlignedBuf::<u8, 2>::new();
        assert!(buf.try_push(1));
        assert!(buf.try_push(2));
        assert!(buf.is_full());
        assert!(!buf.try_push(3));
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn from_array_round_trip() {
        let arr = [1u32, 2, 3, 4];
        let buf = AlignedBuf::<u32, 4>::from_array(arr);
        assert_eq!(buf.as_slice(), &[1, 2, 3, 4]);
        let arr2: [u32; 4] = buf.into();
        assert_eq!(arr2, [1, 2, 3, 4]);
    }

    #[test]
    fn filled_and_clear() {
        let mut buf = AlignedBuf::<f32, 8>::filled(3.14);
        assert_eq!(buf.len(), 8);
        buf.clear();
        assert!(buf.is_empty());
    }

    #[test]
    fn truncate() {
        let mut buf = AlignedBuf::<u64, 6>::filled(0);
        buf.truncate(3);
        assert_eq!(buf.len(), 3);
        buf.truncate(10); // no-op
        assert_eq!(buf.len(), 3);
    }

    #[test]
    fn zero_fill() {
        let mut buf = AlignedBuf::<u32, 4>::filled(0xDEAD_BEEF);
        buf.zero_fill();
        assert!(buf.as_slice().iter().all(|&x| x == 0));
    }

    #[test]
    fn deref_slice_ops() {
        let mut buf = AlignedBuf::<i32, 5>::new();
        for i in 0..5i32 {
            buf.push(i);
        }
        // via Deref
        assert_eq!(buf.iter().copied().sum::<i32>(), 10);
        assert_eq!(buf[2], 2);
    }

    #[test]
    fn from_slice_truncating() {
        let src = [1u32, 2, 3, 4, 5, 6];
        let buf = AlignedBuf::<u32, 4>::from_slice_truncating(&src);
        assert_eq!(buf.as_slice(), &[1, 2, 3, 4]);
    }

    #[test]
    fn clone_and_copy() {
        let mut buf = AlignedBuf::<u8, 4>::new();
        buf.push(1);
        buf.push(2);
        let copy = buf;
        let clone = buf.clone();
        assert_eq!(copy.as_slice(), &[1, 2]);
        assert_eq!(clone.as_slice(), &[1, 2]);
    }

    #[test]
    fn partial_eq() {
        let mut a = AlignedBuf::<u32, 4>::new();
        let mut b = AlignedBuf::<u32, 4>::new();
        a.push(1); a.push(2);
        b.push(1); b.push(2);
        assert_eq!(a, b);
        b.push(3);
        assert_ne!(a, b);
    }

    #[test]
    fn from_iter_truncates() {
        let buf: AlignedBuf<u32, 3> = (0u32..10).collect();
        assert_eq!(buf.as_slice(), &[0, 1, 2]);
    }

    #[test]
    fn const_new_in_static() {
        static BUF: AlignedBuf<u32, 8> = AlignedBuf::new();
        assert!(BUF.is_empty());
        assert_eq!(BUF.capacity(), 8);
    }

    #[test]
    fn remaining() {
        let mut buf = AlignedBuf::<u8, 4>::new();
        assert_eq!(buf.remaining(), 4);
        buf.push(0);
        assert_eq!(buf.remaining(), 3);
    }
}
