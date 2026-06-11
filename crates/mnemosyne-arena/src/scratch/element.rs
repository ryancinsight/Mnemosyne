/// Default alignment for scratch buffers (64 bytes = one AVX-512 cache line).
pub const DEFAULT_SCRATCH_ALIGN: usize = 64;

mod sealed {
    pub trait ScratchElementSealed {}
}

/// Element types that the scratch pool can manage.
///
/// Implemented for `f32`, `f64`, `u8`, and (with the `num-complex` feature)
/// `num_complex::Complex32` and `num_complex::Complex64`. The trait is sealed
/// so new implementations cannot be added downstream.
pub trait ScratchElement: sealed::ScratchElementSealed + Copy + Send + Sync + 'static {
    /// Alignment in bytes required for SIMD operations on this element type.
    const ALIGN_BYTES: usize;
}

impl sealed::ScratchElementSealed for f32 {}
impl ScratchElement for f32 {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

impl sealed::ScratchElementSealed for f64 {}
impl ScratchElement for f64 {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

impl sealed::ScratchElementSealed for u8 {}
impl ScratchElement for u8 {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

#[cfg(feature = "num-complex")]
impl sealed::ScratchElementSealed for num_complex::Complex32 {}
#[cfg(feature = "num-complex")]
impl ScratchElement for num_complex::Complex32 {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

#[cfg(feature = "num-complex")]
impl sealed::ScratchElementSealed for num_complex::Complex64 {}
#[cfg(feature = "num-complex")]
impl ScratchElement for num_complex::Complex64 {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

/// Default alignment constant for external consumers.
#[inline]
pub const fn default_align() -> usize {
    DEFAULT_SCRATCH_ALIGN
}
