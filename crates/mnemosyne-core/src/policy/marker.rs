//! [`PolicyMarker<P>`] — a zero-sized typestate brand carrying the allocator
//! policy as a compile-time proof.

use super::alloc_policy::AllocPolicy;

/// Zero-sized type (ZST) that brands a data structure with the allocator
/// policy used to create it — zero runtime cost (`PhantomData<P>`).
///
/// Embed `PolicyMarker<P>` in consumer types to carry the policy as a
/// compile-time proof without any runtime overhead.
pub struct PolicyMarker<P: AllocPolicy>(core::marker::PhantomData<P>);

impl<P: AllocPolicy> PolicyMarker<P> {
    /// Creates a new zero-cost marker.
    #[inline]
    pub const fn new() -> Self {
        Self(core::marker::PhantomData)
    }

    /// Returns the compile-time policy name.
    #[inline]
    pub const fn name() -> &'static str {
        P::POLICY_NAME
    }

    /// Returns the compile-time policy fingerprint.
    #[inline]
    pub const fn fingerprint() -> u64 {
        P::POLICY_FINGERPRINT
    }

    /// Returns the compile-time mitigation flags bitmask.
    #[inline]
    pub const fn mitigation_flags() -> u32 {
        P::MITIGATION_FLAGS
    }
}

impl<P: AllocPolicy> Default for PolicyMarker<P> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<P: AllocPolicy> Clone for PolicyMarker<P> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<P: AllocPolicy> Copy for PolicyMarker<P> {}

impl<P: AllocPolicy> core::fmt::Debug for PolicyMarker<P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "PolicyMarker<{}>", P::POLICY_NAME)
    }
}

impl<P: AllocPolicy> PartialEq for PolicyMarker<P> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl<P: AllocPolicy> Eq for PolicyMarker<P> {}

const _: () = assert!(
    core::mem::size_of::<PolicyMarker<super::StandardPolicy>>() == 0,
    "PolicyMarker must be a ZST"
);
