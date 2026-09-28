//! Byte, `Pod`, and UTF-8 string views over an [`AlignedVec`]'s initialized
//! prefix.
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

/// `AlignedVec<u8>` as a `core::fmt::Write` sink.
///
/// Enables zero-allocation formatted output into an aligned buffer:
///
/// ```rust
/// use core::fmt::Write as _;
/// use mnemosyne_arena::AlignedVec;
///
/// let mut buf = AlignedVec::<u8>::with_capacity(64);
/// write!(buf, "hello {}", 42).unwrap();
/// assert_eq!(buf.as_slice(), b"hello 42");
/// ```
impl core::fmt::Write for AlignedVec<u8> {
    /// Appends the UTF-8 bytes of `s` to the buffer, growing if needed.
    #[inline]
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

/// Formats an `AlignedVec<u8>` as a UTF-8 string (lossy).
///
/// Non-UTF-8 bytes are replaced with the Unicode replacement character U+FFFD.
impl core::fmt::Display for AlignedVec<u8> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match core::str::from_utf8(self.as_slice()) {
            Ok(s) => f.write_str(s),
            Err(_) => {
                // Replace non-UTF-8 bytes with U+FFFD replacement character.
                let s = alloc::string::String::from_utf8_lossy(self.as_slice());
                f.write_str(&s)
            }
        }
    }
}

impl From<&str> for AlignedVec<u8> {
    /// Copies the bytes of `s` into a new buffer.
    #[inline]
    fn from(s: &str) -> Self {
        Self::from_slice(s.as_bytes())
    }
}

impl AlignedVec<u8> {
    /// Appends the bytes of `s` to the buffer.
    ///
    /// Equivalent to `self.extend_from_slice(s.as_bytes())` but named for
    /// discoverability alongside the [`From<&str>`][From] impl and the
    /// [`core::fmt::Write`] impl.
    #[inline]
    pub fn push_str(&mut self, s: &str) {
        self.extend_from_slice(s.as_bytes());
    }

    /// Interprets the initialized bytes as a UTF-8 string slice.
    ///
    /// Returns `Err` if the bytes are not valid UTF-8.
    #[inline]
    pub fn as_str(&self) -> Result<&str, core::str::Utf8Error> {
        core::str::from_utf8(self.as_slice())
    }

    /// Interprets the initialized bytes as a UTF-8 string, replacing invalid
    /// sequences with U+FFFD.
    #[inline]
    #[must_use]
    pub fn to_string_lossy(&self) -> alloc::borrow::Cow<'_, str> {
        alloc::string::String::from_utf8_lossy(self.as_slice())
    }
}
