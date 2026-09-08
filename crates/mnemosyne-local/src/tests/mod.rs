#![cfg_attr(
    test,
    expect(
        clippy::unwrap_used,
        reason = "MNEM-UNWRAP-1: test scope, an unmet precondition in a test is a test failure"
    )
)]

use super::*;
use core::ptr::NonNull;
use mnemosyne_backend::MemoryBackendWrapper;
use mnemosyne_core::constants::{
    MAX_ALLOC_SIZE, PAGE_SHIFT, PAGE_SIZE, PAGES_PER_SEGMENT, SEGMENT_SIZE,
};
use mnemosyne_core::policy::StandardPolicy;
use mnemosyne_core::types::{Block, Segment};

mod allocation;
mod corruption;
mod double_free;
mod usable_size;
mod validation;
