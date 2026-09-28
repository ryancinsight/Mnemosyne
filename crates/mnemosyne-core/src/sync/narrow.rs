//! 32-bit head codec for [super::AtomicFreeList].
//!
//! On 32-bit targets there is no room to pack a push counter into the head word
//! alongside the block address, so the head is a bare pointer and the count
//! comes from walking the detached chain. This is O(k), acceptable because
//! 32-bit targets (primarily WASM) do not run under the same throughput
//! pressure as 64-bit hosts.
//!
//! Compiled only under `target_pointer_width != "64"`; on a 64-bit host
//! [`super::wide::PackedHead`] is selected instead, so this codec is verified by
//! a 32-bit/WASM build rather than by the host test suite.

use super::HeadCodec;
use crate::loom_shim::{AtomicPtr, Ordering};
use crate::types::Block;

/// Head codec for 32-bit targets: a bare pointer, counted by walking.
pub(super) struct BareHead;

impl HeadCodec for BareHead {
    const COUNT_REQUIRES_WALK: bool = true;

    #[inline(always)]
    fn load(head: &AtomicPtr<Block>, order: Ordering) -> *mut Block {
        head.load(order)
    }

    #[inline(always)]
    fn cas(
        head: &AtomicPtr<Block>,
        current: *mut Block,
        next: *mut Block,
        success: Ordering,
        failure: Ordering,
    ) -> Result<*mut Block, *mut Block> {
        head.compare_exchange_weak(current, next, success, failure)
    }

    #[inline(always)]
    fn swap_null(head: &AtomicPtr<Block>, order: Ordering) -> *mut Block {
        head.swap(core::ptr::null_mut(), order)
    }

    #[inline(always)]
    fn addr(raw: *mut Block) -> *mut Block {
        raw
    }

    #[inline(always)]
    fn assert_packable(_addr: *mut Block) {}

    #[inline(always)]
    fn pack(addr: *mut Block, _current: *mut Block) -> *mut Block {
        addr
    }

    #[inline(always)]
    fn validate(_raw: *mut Block) {}

    #[inline(always)]
    fn count_of(_raw: *mut Block, walked: Option<usize>) -> usize {
        match walked {
            Some(walked) => walked,
            None => crate::abort::abort_on_corruption("bare head codec requires a chain walk"),
        }
    }
}
