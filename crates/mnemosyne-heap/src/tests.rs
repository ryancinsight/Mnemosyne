#![allow(clippy::missing_const_for_thread_local)]
extern crate std;
use super::*;
use core::alloc::Layout;
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_backend::MemoryBackendWrapper;
use mnemosyne_core::StandardPolicy;
use std::format;

fn test_layout(size: usize, align: usize) -> Layout {
    Layout::from_size_align(size, align)
        .expect("heap unit test layout must use a nonzero power-of-two alignment")
}

#[derive(Debug)]
struct DropTracker<'a>(&'a AtomicUsize);
impl<'a> Drop for DropTracker<'a> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

std::thread_local! {
    static ZST_DROP_COUNT: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

#[derive(Debug)]
struct ZstDrop;

impl Drop for ZstDrop {
    fn drop(&mut self) {
        ZST_DROP_COUNT.with(|c| c.set(c.get() + 1));
    }
}

mod boxed;
mod cell;
mod heap;
mod tiered;
mod traits;
mod vec;
