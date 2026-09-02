//! Element types admitted into scratch buffers: sealed plain-old-data
//! scalars for which an all-zero bit pattern is a valid value.

/// Default alignment for scratch buffers (64 bytes = one AVX-512 cache line).
pub const DEFAULT_SCRATCH_ALIGN: usize = 64;

mod sealed {
    pub trait ScratchElementSealed {}
}

/// Element types that the scratch pool can manage.
///
/// Implemented for `f32`, `f64`, `u8`, common integer types, and (with the
/// `eunomia` feature) `eunomia::Complex<f32>` and `eunomia::Complex<f64>`.
/// The trait is sealed so new implementations cannot be added downstream.
///
/// # Safety invariant
///
/// Every implementor must tolerate an all-zero bit pattern as a valid,
/// non-trapping value. This is true for all floating-point scalars (±0.0),
/// all integers (0), and eunomia complex numbers. Types with validity
/// invariants (e.g. non-null pointers, Rust references, enums with niche
/// optimizations) must not implement this trait.
pub trait ScratchElement: sealed::ScratchElementSealed + Copy + Send + Sync + 'static {
    /// Alignment in bytes required for SIMD operations on this element type.
    const ALIGN_BYTES: usize;
}

macro_rules! impl_scratch_element {
    ($($t:ty),* $(,)?) => {
        $(
            impl sealed::ScratchElementSealed for $t {}
            impl ScratchElement for $t {
                const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
            }
        )*
    };
}

impl_scratch_element!(f32, f64, u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

#[cfg(feature = "eunomia")]
impl sealed::ScratchElementSealed for eunomia::Complex<f32> {}
#[cfg(feature = "eunomia")]
impl ScratchElement for eunomia::Complex<f32> {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

#[cfg(feature = "eunomia")]
impl sealed::ScratchElementSealed for eunomia::Complex<f64> {}
#[cfg(feature = "eunomia")]
impl ScratchElement for eunomia::Complex<f64> {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

/// Default alignment constant for external consumers.
#[inline]
pub const fn default_align() -> usize {
    DEFAULT_SCRATCH_ALIGN
}
