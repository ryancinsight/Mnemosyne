//! Per-thread allocation counting for allocator-budget tests.
//!
//! [`CountingAllocator`] wraps any [`GlobalAlloc`] and records, per thread, how
//! many allocations, reallocations and deallocations that thread performed and
//! how many bytes they moved. [`measure`] runs a closure and returns the
//! counts the calling thread produced while it ran.
//!
//! A process-wide counter is the wrong instrument for a test that asserts an
//! exact allocation budget: the test harness runs the test body on a spawned
//! thread while its main thread keeps allocating for its own bookkeeping, and
//! parallel tests allocate concurrently. Both land inside a process-wide
//! window and fail it for reasons unrelated to the code under test. Counting
//! per thread removes both sources without serializing the test run.
//!
//! The counters are not part of the wrapped allocator, so wrapping
//! [`Mnemosyne`](crate::Mnemosyne) measures exactly the allocator a program
//! ships with.
//!
//! # Semantics
//!
//! - **Per-thread.** Each thread has its own counters and its own set of open
//!   windows. [`measure`] reads only the calling thread's counters, so an
//!   allocation made by any other thread — including one the measured body
//!   waits on — never enters the window. There is no cross-thread total.
//! - **Reentrancy.** Recording touches only const-initialized thread-local
//!   `Cell`s: it allocates nothing, locks nothing, and cannot re-enter the
//!   allocator it instruments, so it is safe on every allocator call path,
//!   including one reached from inside another allocation. Windows nest: an
//!   inner [`measure`] composes into the outer window's counts, and if the
//!   measured body panics, the window is closed as the panic unwinds past
//!   it, so no later window on that thread is affected.
//! - **Thread exit.** The counters live in thread-local storage with no drop
//!   glue and are discarded with the thread. A thread that exits with a
//!   window still open (only possible by unwinding out of it) takes its
//!   counts with it; nothing is reported anywhere else, and a later thread
//!   starts from zero.
//! - **When not measuring.** Recording is gated on the calling thread having
//!   a [`measure`] window open. Outside one, an allocator call through the
//!   wrapper adds one thread-local load and one branch and no stores, so
//!   unmeasured traffic pays no counter traffic. A binary that does not
//!   install the wrapper at all pays literally nothing: the counter type is
//!   the size of the allocator it wraps, and production binaries install
//!   their allocator unwrapped.
//!
//! # Example
//!
//! ```
//! use mnemosyne::Mnemosyne;
//! use mnemosyne::counting::{CountingAllocator, measure};
//!
//! #[global_allocator]
//! static ALLOCATOR: CountingAllocator<Mnemosyne> = CountingAllocator::new(Mnemosyne);
//!
//! fn main() {
//!     let (boxed, delta) = measure(|| std::hint::black_box(Box::new(7_u64)));
//!     assert_eq!(*boxed, 7);
//!     assert_eq!(delta.allocations, 1);
//!     assert_eq!(delta.bytes_allocated, 8);
//!     assert_eq!(delta.deallocations, 0);
//! }
//! ```

use core::alloc::{GlobalAlloc, Layout};
use core::cell::Cell;

/// Counts of allocator calls and the bytes they moved.
///
/// Every field is an exact tally of what the calling thread did, modulo
/// `usize::MAX + 1` (the counters wrap rather than panic, because a panic
/// inside a global allocator aborts the process). A window that moves fewer
/// than `usize::MAX` bytes reads exactly.
///
/// `Default` is the empty delta, the value a window that never touched the
/// heap reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AllocationDelta {
    /// Successful `alloc` and `alloc_zeroed` calls.
    pub allocations: usize,
    /// `dealloc` calls.
    pub deallocations: usize,
    /// Successful `realloc` calls.
    pub reallocations: usize,
    /// Bytes acquired: the size of every successful allocation, plus the growth
    /// of every successful `realloc` to a larger size.
    pub bytes_allocated: usize,
    /// Bytes released: the size of every deallocated block, plus the shrinkage
    /// of every successful `realloc` to a smaller size.
    pub bytes_deallocated: usize,
}

impl AllocationDelta {
    /// Net heap growth over the window: [`Self::bytes_allocated`] minus
    /// [`Self::bytes_deallocated`], signed.
    ///
    /// Positive when the window retained memory, negative when it released
    /// memory acquired before the window began, zero when it was balanced.
    /// Exact while the magnitude is below `isize::MAX`.
    #[must_use]
    pub const fn bytes_retained(&self) -> isize {
        self.bytes_allocated
            .wrapping_sub(self.bytes_deallocated)
            .cast_signed()
    }

    /// Counts accumulated since `earlier`, a snapshot taken on the same thread.
    const fn since(&self, earlier: &Self) -> Self {
        Self {
            allocations: self.allocations.wrapping_sub(earlier.allocations),
            deallocations: self.deallocations.wrapping_sub(earlier.deallocations),
            reallocations: self.reallocations.wrapping_sub(earlier.reallocations),
            bytes_allocated: self.bytes_allocated.wrapping_sub(earlier.bytes_allocated),
            bytes_deallocated: self
                .bytes_deallocated
                .wrapping_sub(earlier.bytes_deallocated),
        }
    }
}

/// One thread's running totals. Plain `Cell`s: only the owning thread touches
/// them, so no synchronization exists to cost anything or to recurse.
///
/// `open_windows` is the recording gate: [`measure`] raises it for the
/// duration of its window (nested windows raise it further), and the
/// `record_*` functions touch the tallies only while it is non-zero. Traffic
/// outside any window therefore costs one load and one branch per allocator
/// call and no stores.
struct Counters {
    allocations: Cell<usize>,
    deallocations: Cell<usize>,
    reallocations: Cell<usize>,
    bytes_allocated: Cell<usize>,
    bytes_deallocated: Cell<usize>,
    open_windows: Cell<usize>,
}

impl Counters {
    const fn new() -> Self {
        Self {
            allocations: Cell::new(0),
            deallocations: Cell::new(0),
            reallocations: Cell::new(0),
            bytes_allocated: Cell::new(0),
            bytes_deallocated: Cell::new(0),
            open_windows: Cell::new(0),
        }
    }

    fn totals(&self) -> AllocationDelta {
        AllocationDelta {
            allocations: self.allocations.get(),
            deallocations: self.deallocations.get(),
            reallocations: self.reallocations.get(),
            bytes_allocated: self.bytes_allocated.get(),
            bytes_deallocated: self.bytes_deallocated.get(),
        }
    }
}

// The slot is const-initialized and its type has no drop glue, so reading it
// runs no user code and never allocates through the global allocator, which
// would re-enter the allocator being instrumented. It stays readable for the
// whole life of the thread, for a different reason on each storage kind in the
// standard library (`sys/thread_local`, Rust 1.97):
//
// - Native thread-local storage registers no destructor for a type without
//   drop glue, so nothing ever marks the slot unreadable.
// - The OS-key fallback registers a destructor for every thread-local and
//   boxes the slot through `System`, not the global allocator, so first access
//   does not re-enter it. The destructor marks the slot unreadable only while
//   it drops the value, which has no glue and calls no method of this
//   allocator, and resets the slot afterwards, so a read after teardown
//   reinitializes it instead of failing.
std::thread_local! {
    static COUNTERS: Counters = const { Counters::new() };
}

fn bump(cell: &Cell<usize>, by: usize) {
    cell.set(cell.get().wrapping_add(by));
}

fn record_alloc(size: usize) {
    COUNTERS.with(|counters| {
        if counters.open_windows.get() == 0 {
            return;
        }
        bump(&counters.allocations, 1);
        bump(&counters.bytes_allocated, size);
    });
}

fn record_dealloc(size: usize) {
    COUNTERS.with(|counters| {
        if counters.open_windows.get() == 0 {
            return;
        }
        bump(&counters.deallocations, 1);
        bump(&counters.bytes_deallocated, size);
    });
}

fn record_realloc(old_size: usize, new_size: usize) {
    COUNTERS.with(|counters| {
        if counters.open_windows.get() == 0 {
            return;
        }
        bump(&counters.reallocations, 1);
        if new_size >= old_size {
            bump(&counters.bytes_allocated, new_size - old_size);
        } else {
            bump(&counters.bytes_deallocated, old_size - new_size);
        }
    });
}

/// One open [`measure`] window on the current thread. Dropping it closes the
/// window, so a body that panics still restores the gate as it unwinds.
struct OpenWindow;

impl OpenWindow {
    fn enter() -> Self {
        COUNTERS.with(|counters| bump(&counters.open_windows, 1));
        Self
    }
}

impl Drop for OpenWindow {
    fn drop(&mut self) {
        COUNTERS.with(|counters| {
            counters
                .open_windows
                .set(counters.open_windows.get().saturating_sub(1));
        });
    }
}

/// A [`GlobalAlloc`] that forwards to `A` and counts, per calling thread, what
/// it forwarded.
///
/// Install it as the `#[global_allocator]` of the test binary whose
/// allocation budget is under test, then read the counts with [`measure`].
/// Only calls that succeed are counted: a null return from the inner
/// allocator acquired nothing and is not an allocation.
///
/// Inside a [`measure`] window, counting costs two `Cell` updates on
/// thread-local counters per call; outside one it costs one thread-local
/// load and one branch, with no stores. The wrapper is exactly the size of
/// `A`. It is a test instrument; production binaries install their allocator
/// unwrapped and pay nothing.
#[derive(Debug, Default)]
pub struct CountingAllocator<A> {
    inner: A,
}

impl<A> CountingAllocator<A> {
    /// Wraps `inner`. `const`, so the wrapper can initialize a `static`.
    #[must_use]
    pub const fn new(inner: A) -> Self {
        Self { inner }
    }
}

// SAFETY: every method forwards to `A`, which upholds the `GlobalAlloc`
// contract, with the caller's pointer and layout unmodified and returns its
// result unmodified. The added effect is a `Cell` update on a const-initialized
// thread-local, which neither allocates nor panics, so it cannot re-enter the
// allocator or unwind out of it.
unsafe impl<A: GlobalAlloc> GlobalAlloc for CountingAllocator<A> {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller upholds `alloc`'s contract for `layout`, which is
        // forwarded verbatim.
        let ptr = unsafe { self.inner.alloc(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size());
        }
        ptr
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller upholds `dealloc`'s contract: `ptr` was returned
        // by this allocator for `layout`, and both are forwarded verbatim.
        unsafe { self.inner.dealloc(ptr, layout) };
        record_dealloc(layout.size());
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller upholds `alloc_zeroed`'s contract for `layout`,
        // which is forwarded verbatim so the inner allocator keeps its own
        // zeroing path.
        let ptr = unsafe { self.inner.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size());
        }
        ptr
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller upholds `realloc`'s contract: `ptr` was returned
        // by this allocator for `layout` and `new_size` is valid for its
        // alignment, all forwarded verbatim so the inner allocator keeps its
        // own in-place path.
        let new_ptr = unsafe { self.inner.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            record_realloc(layout.size(), new_size);
        }
        new_ptr
    }
}

/// Runs `body` and returns its value with the allocator traffic the calling
/// thread produced while it ran.
///
/// Allocations made by other threads are not counted, including threads
/// `body` waits on. Allocations `body` makes to start a thread (its closure,
/// its handle) are made by the calling thread and are counted.
///
/// Counts are read by difference, so windows nest: an outer window includes
/// the traffic of an inner one. Counting happens only where a
/// [`CountingAllocator`] is installed as the global allocator; without one
/// every window reads empty.
///
/// # Example
///
/// ```
/// use mnemosyne::counting::{AllocationDelta, CountingAllocator, measure};
///
/// #[global_allocator]
/// static ALLOCATOR: CountingAllocator<std::alloc::System> =
///     CountingAllocator::new(std::alloc::System);
///
/// fn main() {
///     let mut buffer = Vec::<u8>::with_capacity(16);
///     let ((), delta) = measure(|| buffer.extend_from_slice(&[0; 16]));
///     assert_eq!(delta, AllocationDelta::default());
///
///     let ((), delta) = measure(|| buffer.extend_from_slice(&[0; 16]));
///     assert_eq!(delta.reallocations, 1);
///     assert_eq!(delta.bytes_allocated, buffer.capacity() - 16);
/// }
/// ```
pub fn measure<R>(body: impl FnOnce() -> R) -> (R, AllocationDelta) {
    // Open the window before the baseline snapshot so the snapshot itself is
    // the first counted instant, and hold it in a guard so a panicking body
    // closes it on unwind rather than leaving recording on for this thread.
    let _window = OpenWindow::enter();
    let before = COUNTERS.with(Counters::totals);
    let value = body();
    let after = COUNTERS.with(Counters::totals);
    (value, after.since(&before))
}

#[cfg(test)]
mod tests {
    use super::{AllocationDelta, COUNTERS, Counters, measure, record_alloc};

    fn totals() -> AllocationDelta {
        COUNTERS.with(Counters::totals)
    }

    #[test]
    fn recording_is_inert_outside_a_window() {
        let before = totals();
        record_alloc(24);
        assert_eq!(totals(), before);

        let ((), delta) = measure(|| record_alloc(24));
        assert_eq!(
            delta,
            AllocationDelta {
                allocations: 1,
                bytes_allocated: 24,
                ..AllocationDelta::default()
            }
        );

        record_alloc(24);
        assert_eq!(
            totals(),
            AllocationDelta {
                allocations: before.allocations + 1,
                bytes_allocated: before.bytes_allocated + 24,
                ..before
            }
        );
    }

    #[test]
    fn closing_an_inner_window_keeps_the_outer_one_recording() {
        let ((), outer) = measure(|| {
            let ((), inner) = measure(|| record_alloc(8));
            assert_eq!(inner.allocations, 1);
            record_alloc(8);
        });
        assert_eq!(outer.allocations, 2);
        assert_eq!(outer.bytes_allocated, 16);
    }

    #[test]
    fn a_panicking_body_closes_its_window() {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let ((), _) = measure(|| -> () { panic!("invariant: test body") });
        }));
        let Err(payload) = outcome else {
            panic!("invariant: the panicking body unwinds out of measure");
        };
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"invariant: test body")
        );

        let before = totals();
        record_alloc(24);
        assert_eq!(totals(), before);
    }
}
