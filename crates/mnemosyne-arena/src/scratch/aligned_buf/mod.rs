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

mod ops;
mod traits;

use crate::scratch::element::ScratchElement;
use core::mem::MaybeUninit;

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
// `ScratchElement` already requires `Copy`, so the derived bounds add no
// constraint an instantiation could fail. Deriving keeps the copy bitwise
// rather than looping over the initialized prefix.
#[derive(Clone, Copy)]
pub struct AlignedBuf<T: ScratchElement, const N: usize> {
    /// Inline storage for up to `N` elements.
    data: [MaybeUninit<T>; N],
    /// Number of initialized elements.
    len: usize,
}

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
        let mut buf = AlignedBuf::<f32, 8>::filled(core::f32::consts::PI);
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
        let clone = buf;
        assert_eq!(copy.as_slice(), &[1, 2]);
        assert_eq!(clone.as_slice(), &[1, 2]);
    }

    #[test]
    fn partial_eq() {
        let mut a = AlignedBuf::<u32, 4>::new();
        let mut b = AlignedBuf::<u32, 4>::new();
        a.push(1);
        a.push(2);
        b.push(1);
        b.push(2);
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
