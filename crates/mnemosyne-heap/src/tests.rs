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

/// An element whose destructor panics, for checking that a container still
/// returns its block on the unwind path.
///
/// Non-ZST on purpose: a ZST never gets a block, so only a sized payload
/// exercises the free. Containers under test hold exactly one of these — a
/// second panicking destructor would run during the first one's unwind and
/// abort the process rather than fail the test.
#[derive(Debug)]
struct PanicOnDrop(u64);

impl Drop for PanicOnDrop {
    fn drop(&mut self) {
        std::panic!("element {} destructor panics by design", self.0);
    }
}

/// Runs `f`, reporting whether it unwound, with the panic hook silenced so the
/// deliberate panic does not bury the run's real output.
fn catch_expected_panic(f: impl FnOnce()) -> bool {
    let previous = std::panic::take_hook();
    std::panic::set_hook(std::boxed::Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    std::panic::set_hook(previous);
    outcome.is_err()
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
