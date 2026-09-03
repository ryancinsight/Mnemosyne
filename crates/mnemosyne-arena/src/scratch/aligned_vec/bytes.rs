//! Byte and `Pod` views over an [`AlignedVec`]'s initialized prefix.
//!
//! These reinterpret what is already there and never change the length, so
//! they sit apart from the operations that do.

use super::AlignedVec;
use super::ScratchElement;

impl<T: ScratchElement> AlignedVec<T> {
    /// Zero-copy view of the initialized elements as raw bytes.
    ///
    /// Available with `features = ["bytemuck"]` because this method requires
    /// `T: bytemuck::Pod` — the guarantee that no byte in the element's
    /// representation is uninitialized (padding bytes are not Pod-safe).
    ///
    /// The result length is `self.len() * size_of::<T>()`.
    #[cfg(feature = "bytemuck")]
    #[inline]
    pub fn as_bytes(&self) -> &[u8]
    where
        T: bytemuck::Pod,
    {
        bytemuck::cast_slice(self.as_slice())
    }

    /// Zero-copy mutable view of the initialized elements as raw bytes.
    ///
    /// See [`as_bytes`][Self::as_bytes] for the requirements and the
    /// relationship between the returned slice length and `len()`.
    #[cfg(feature = "bytemuck")]
    #[inline]
    pub fn as_bytes_mut(&mut self) -> &mut [u8]
    where
        T: bytemuck::Pod,
    {
        bytemuck::cast_slice_mut(self.as_mut_slice())
    }

    /// Zero-copy reinterpretation of the initialized elements as a slice of
    /// a different type `U`.
    ///
    /// Available with `features = ["bytemuck"]`. Both `T` and `U` must be
    /// `bytemuck::Pod`. The call panics when `size_of::<T>() * len()` is not
    /// a multiple of `size_of::<U>()` — the same contract as
    /// `bytemuck::cast_slice`.
    ///
    /// # Use cases
    ///
    /// - View `AlignedVec<Complex32>` (interleaved re/im as f32 pairs) as
    ///   `&[f32]` for partial in-place transforms or GPU upload.
    /// - View `AlignedVec<u32>` GPU index data as `&[u8]` for zero-copy
    ///   DMA staging, without an intermediate `Vec<u8>` copy.
    #[cfg(feature = "bytemuck")]
    #[inline]
    pub fn cast_slice<U: bytemuck::Pod>(&self) -> &[U]
    where
        T: bytemuck::Pod,
    {
        bytemuck::cast_slice(self.as_slice())
    }

    /// Zero-copy mutable reinterpretation of the initialized elements as `U`.
    ///
    /// See [`cast_slice`][Self::cast_slice] for requirements and panics.
    #[cfg(feature = "bytemuck")]
    #[inline]
    pub fn cast_slice_mut<U: bytemuck::Pod>(&mut self) -> &mut [U]
    where
        T: bytemuck::Pod,
    {
        bytemuck::cast_slice_mut(self.as_mut_slice())
    }
}
