//! Compile-time security mitigation bitmask constants.
//!
//! Combine with bitwise OR to describe a policy's active mitigations.
//! Inspired by snmalloc 0.7.x `mitigations/mitigations.h`.

/// Poison freed and allocated bytes with sentinel patterns.
pub const POISONING: u32 = 1 << 0;
/// Zero-initialize all allocations.
pub const ZERO_INIT: u32 = 1 << 1;
/// XOR-encrypt free-list next-pointers per page cookie.
pub const FREE_LIST_ENCRYPTION: u32 = 1 << 2;
/// Fisher-Yates–shuffle free list on page initialization.
pub const RANDOMIZE_ALLOCATION: u32 = 1 << 3;
/// Hysteresis: delay page waking until ≥ capacity/WAKE_DENOMINATOR freed.
pub const DELAY_PAGE_WAKE: u32 = 1 << 4;
/// Multiplicative backward-edge canary on freed blocks.
pub const FREE_CANARY: u32 = 1 << 5;
/// Validate caller's size/align against stored block_size on sized free.
pub const SIZED_FREE_VALIDATION: u32 = 1 << 6;
/// Free-canary is now wired: it IS enforced at runtime.
pub const FREE_CANARY_WIRED: u32 = 1 << 7;
/// Dual free-list per page (random-preserve): each page maintains a second
/// free list so that the allocator can randomly select from two lists,
/// widening the temporal distance between consecutive free and alloc of the
/// same block.
///
/// Inspired by snmalloc 0.7.x `random_preserve` proposal. This bit is
/// defined for future use — a full page-struct extension is needed to
/// activate it.
pub const DUAL_FREELIST: u32 = 1 << 8;
/// Mitigations with an end-to-end runtime implementation.
///
/// `DELAY_PAGE_WAKE` and `FREE_CANARY_WIRED` belong here.
/// `SIZED_FREE_VALIDATION` stays out — helpers exist but no production
/// caller reaches them yet.
pub const IMPLEMENTED: u32 = POISONING
    | ZERO_INIT
    | FREE_LIST_ENCRYPTION
    | RANDOMIZE_ALLOCATION
    | DELAY_PAGE_WAKE
    | FREE_CANARY
    | FREE_CANARY_WIRED;
/// Every mitigation bit defined by this registry.
///
/// This is a registry mask, not a claim that every bit is active in a
/// policy. Use [`IMPLEMENTED`] or a policy's
/// [`MITIGATION_FLAGS`][super::AllocPolicy::MITIGATION_FLAGS]
/// for the currently enforced data-plane mitigations.
pub const ALL: u32 = IMPLEMENTED | SIZED_FREE_VALIDATION | DUAL_FREELIST;
/// No mitigations.
pub const NONE: u32 = 0;
