//! Element types admitted into scratch buffers: sealed plain-old-data
//! scalars for which an all-zero bit pattern is a valid value.

/// Default alignment for scratch buffers (64 bytes = one AVX-512 cache line).
pub const DEFAULT_SCRATCH_ALIGN: usize = 64;

mod sealed {
    pub trait ScratchElementSealed {}

    // Blanket impl: any Zeroable type has a valid all-zero bit pattern by
    // definition. The sealed trait prevents non-Zeroable downstream impls.
    #[cfg(feature = "bytemuck")]
    impl<T: bytemuck::Zeroable + Copy + Send + Sync + 'static> ScratchElementSealed for T {}
}

/// Element types that the scratch pool can manage.
///
/// Implemented for `f32`, `f64`, `u8`, common integer types, and (with the
/// `eunomia` feature) `eunomia::Complex<f32>` and `eunomia::Complex<f64>`.
/// With the `bytemuck` feature, a blanket impl covers any
/// `bytemuck::Zeroable + Copy + Send + Sync + 'static` type — including
/// arbitrary repr(C) POD structs that derive `Zeroable`.
///
/// # Safety invariant
///
/// Every implementor must tolerate an all-zero bit pattern as a valid,
/// non-trapping value. This is true for all floating-point scalars (±0.0),
/// all integers (0), eunomia complex numbers, and any type that implements
/// `bytemuck::Zeroable`. Types with validity invariants (e.g. non-null
/// pointers, Rust references, enums with niche optimizations) must not
/// implement this trait.
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

// Manual impls when the bytemuck blanket is not active.
// With the `bytemuck` feature, the blanket `impl<T: Zeroable + ...>` below
// covers all of these (plus every other Zeroable POD type).
#[cfg(not(feature = "bytemuck"))]
impl_scratch_element!(f32, f64, bool, u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

#[cfg(all(feature = "eunomia", not(feature = "bytemuck")))]
impl sealed::ScratchElementSealed for eunomia::Complex<f32> {}
#[cfg(all(feature = "eunomia", not(feature = "bytemuck")))]
impl ScratchElement for eunomia::Complex<f32> {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

#[cfg(all(feature = "eunomia", not(feature = "bytemuck")))]
impl sealed::ScratchElementSealed for eunomia::Complex<f64> {}
#[cfg(all(feature = "eunomia", not(feature = "bytemuck")))]
impl ScratchElement for eunomia::Complex<f64> {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

/// Blanket `ScratchElement` impl for arbitrary POD structs.
///
/// Enabled with `features = ["bytemuck"]`. Any type that implements
/// `bytemuck::Zeroable` (i.e. has a valid all-zero representation) and
/// satisfies `Copy + Send + Sync + 'static` can be used with
/// [`AlignedVec`][super::aligned_vec::AlignedVec] and
/// [`ScratchPool`][super::pool::ScratchPool].
///
/// The alignment for structs covered by this impl is the crate-wide
/// `DEFAULT_SCRATCH_ALIGN` (64 bytes). If a type requires a different
/// alignment, provide an explicit `impl ScratchElement` instead.
#[cfg(feature = "bytemuck")]
impl<T: bytemuck::Zeroable + Copy + Send + Sync + 'static> ScratchElement for T {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

/// Default alignment constant for external consumers.
#[inline]
pub const fn default_align() -> usize {
    DEFAULT_SCRATCH_ALIGN
}
