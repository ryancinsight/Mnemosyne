use super::AlignedBuf;
use crate::scratch::element::ScratchElement;
use core::mem::MaybeUninit;

impl<T: ScratchElement, const N: usize> AlignedBuf<T, N> {
    // ── Construction ─────────────────────────────────────────────────────────

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

    // ── Capacity queries ────────────────────────────────────────────────────

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

    // ── Push / pop ───────────────────────────────────────────────────────────

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

    // ── Mutation ─────────────────────────────────────────────────────────────

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

    // ── Slice views ──────────────────────────────────────────────────────────

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
