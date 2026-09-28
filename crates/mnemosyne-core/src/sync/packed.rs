//! Packed tagged-pointer codec shared by the free-list head and the
//! segment-pool head.
//!
//! Two structures pack a 48-bit pointer into the low bits of a machine word and
//! keep a wrapping tag/count in the high bits: the per-page cross-thread
//! deallocation queue ([`super::wide`]) and the segment-pool stack
//! (`mnemosyne-arena`'s `TaggedHead`). Both re-derived the same masks from the
//! same `48` and both had to guard the same "address fits in 48 bits" invariant
//! — the former aborting through [`crate::abort::abort_on_corruption`], the
//! latter branching on `std`/`test` into `abort` vs `panic!`. This module is the
//! single extraction of both the bit arithmetic and the invariant.

use core::marker::PhantomData;

/// Bit-manipulation SSOT for a packed `(address, tag)` word.
///
/// `T` only names the pointee; the type is never instantiated (all members are
/// associated). On 64-bit targets the low
/// [`PACKED_PTR_BITS`](Self::PACKED_PTR_BITS) bits hold the address and the
/// remaining high bits hold a wrapping tag/count; on other targets packing is
/// disabled and every operation is the identity.
pub struct PackedTaggedPtr<T>(PhantomData<fn() -> *mut T>);

impl<T> PackedTaggedPtr<T> {
    /// Number of low bits that hold the address in a packed word (64-bit only).
    #[cfg(target_pointer_width = "64")]
    pub const PACKED_PTR_BITS: u32 = 48;

    /// Mask selecting the address bits (64-bit only).
    #[cfg(target_pointer_width = "64")]
    pub const PTR_MASK: usize = (1usize << Self::PACKED_PTR_BITS) - 1;

    /// Mask selecting the tag/count bits (64-bit only).
    #[cfg(target_pointer_width = "64")]
    pub const TAG_MASK: usize = (1usize << (usize::BITS - Self::PACKED_PTR_BITS)) - 1;

    /// The address held in a packed word.
    ///
    /// On non-64-bit targets packing is disabled, so this is the identity.
    #[inline(always)]
    pub fn ptr(state: *mut T) -> *mut T {
        #[cfg(target_pointer_width = "64")]
        {
            state.map_addr(|addr| addr & Self::PTR_MASK)
        }
        #[cfg(not(target_pointer_width = "64"))]
        {
            state
        }
    }

    /// The tag/count held in a packed word's high bits.
    ///
    /// Always `0` where packing is disabled.
    #[inline(always)]
    pub fn tag(state: *mut T) -> usize {
        #[cfg(target_pointer_width = "64")]
        {
            state.addr() >> Self::PACKED_PTR_BITS
        }
        #[cfg(not(target_pointer_width = "64"))]
        {
            let _ = state;
            0
        }
    }

    /// Packs `next`'s address with the tag of `current` advanced by one.
    ///
    /// On non-64-bit targets this is the identity on `next`.
    #[inline(always)]
    pub fn tagged_successor(next: *mut T, current: *mut T) -> *mut T {
        #[cfg(target_pointer_width = "64")]
        {
            let tag = (Self::tag(current) + 1) & Self::TAG_MASK;
            next.map_addr(|addr| (tag << Self::PACKED_PTR_BITS) | (addr & Self::PTR_MASK))
        }
        #[cfg(not(target_pointer_width = "64"))]
        {
            let _ = current;
            next
        }
    }

    /// Returns `addr` unchanged when its whole value fits in the packed address
    /// field, or the reason it does not.
    ///
    /// The single guard for the "address fits in 48 bits" invariant; callers
    /// route the error to the shared corruption sink so the failure policy is
    /// defined once (see [`crate::abort::abort_on_corruption`]).
    #[inline(always)]
    pub fn checked_pack(addr: *mut T) -> Result<*mut T, &'static str> {
        #[cfg(target_pointer_width = "64")]
        {
            if (addr.addr() & !Self::PTR_MASK) != 0 {
                Err("packed head address does not fit in the 48-bit address field")
            } else {
                Ok(addr)
            }
        }
        #[cfg(not(target_pointer_width = "64"))]
        {
            Ok(addr)
        }
    }
}
