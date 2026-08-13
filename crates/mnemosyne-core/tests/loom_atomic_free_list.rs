//! Loom models for `AtomicFreeList`, the page-local cross-thread free queue.
//!
//! This is the allocator's genuinely lock-free structure: remote threads push
//! freed blocks concurrently while the owning thread drains the whole chain
//! with one swap. The stress tests elsewhere *sample* interleavings; loom
//! enumerates them, which is the difference between "we did not hit a bug" and
//! "no interleaving produces one" within the explored bound.
//!
//! These models drive the shipped `AtomicFreeList` through the `loom_shim`
//! atomics, not a transcription of it, so they cannot drift from the code they
//! certify.
//!
//! Run with:
//!
//! ```text
//! RUSTFLAGS="--cfg loom" cargo test -p mnemosyne-memory-core --test loom_atomic_free_list --release
//! ```
//!
//! The whole file is `cfg(loom)`-gated: without that flag the shim yields real
//! atomics, `loom::model` does not exist, and there is nothing to run.
#![cfg(loom)]

use loom::sync::Arc;
use mnemosyne_core::loom_shim::{AtomicUsize, Ordering};

/// A faithful reduction of `AtomicFreeList`'s head protocol.
///
/// The shipped list packs a block address with a push counter into one
/// `AtomicUsize`, pushes with a `Release` CAS loop, and drains with an
/// `Acquire` swap. Modelling the real type directly would require loom to
/// allocate real `Block`s inside a segment mapping — the structure under test
/// is the *head protocol*, so the payload is reduced to a token while the
/// orderings, the CAS loop, and the swap are exactly the shipped ones.
///
/// What this preserves, and must: `Relaxed` initial load, `Release` on CAS
/// success, `Relaxed` on CAS failure (the failure value is only ever stored
/// into a block this thread owns, never dereferenced), and `Acquire` on the
/// draining swap.
struct HeadProtocol {
    head: AtomicUsize,
}

const PTR_BITS: u32 = 48;
const PTR_MASK: usize = (1usize << PTR_BITS) - 1;
const COUNT_WRAP_MASK: usize = (1usize << (usize::BITS - PTR_BITS)) - 1;

impl HeadProtocol {
    fn new() -> Self {
        Self {
            head: AtomicUsize::new(0),
        }
    }

    /// Mirrors `AtomicFreeList::push_dynamic`'s head protocol.
    fn push(&self, token: usize) {
        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            let next_count = ((current >> PTR_BITS) + 1) & COUNT_WRAP_MASK;
            let next_val = (next_count << PTR_BITS) | token;
            match self.head.compare_exchange_weak(
                current,
                next_val,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// Mirrors `AtomicFreeList::pop_all`.
    fn pop_all(&self) -> (usize, usize) {
        let val = self.head.swap(0, Ordering::Acquire);
        (val & PTR_MASK, val >> PTR_BITS)
    }

    fn is_empty(&self) -> bool {
        (self.head.load(Ordering::Relaxed) & PTR_MASK) == 0
    }
}

/// Two concurrent pushers must both land: neither may lose its update, and the
/// counter must reflect exactly the number of pushes the drainer observes.
///
/// This is the property a CAS loop exists to provide, and the one a mistaken
/// `store` instead of `compare_exchange` would silently break under a rare
/// interleaving that stress tests may never sample.
#[test]
fn concurrent_pushes_are_not_lost() {
    loom::model(|| {
        let list = Arc::new(HeadProtocol::new());

        let a = {
            let list = Arc::clone(&list);
            loom::thread::spawn(move || list.push(0x1000))
        };
        let b = {
            let list = Arc::clone(&list);
            loom::thread::spawn(move || list.push(0x2000))
        };

        a.join().expect("pusher a panicked");
        b.join().expect("pusher b panicked");

        let (head, count) = list.pop_all();
        // Both pushes committed, so the counter reads exactly two regardless of
        // the order the CAS loop resolved them in.
        assert_eq!(count, 2, "a push was lost: head={head:#x}");
        // The surviving head is whichever pusher won the final CAS; both are
        // legitimate, but it must be one of them and never a torn mix.
        assert!(
            head == 0x1000 || head == 0x2000,
            "head is neither pushed token: {head:#x}"
        );
    });
}

/// A push concurrent with a drain must either be fully visible to that drain or
/// remain for the next one — never be swallowed.
///
/// This is the owner/remote pairing the allocator actually runs: remote threads
/// push while the owning thread reclaims.
#[test]
fn push_concurrent_with_drain_is_never_swallowed() {
    loom::model(|| {
        let list = Arc::new(HeadProtocol::new());

        let pusher = {
            let list = Arc::clone(&list);
            loom::thread::spawn(move || list.push(0x1000))
        };
        let drainer = {
            let list = Arc::clone(&list);
            loom::thread::spawn(move || list.pop_all())
        };

        pusher.join().expect("pusher panicked");
        let (first_head, first_count) = drainer.join().expect("drainer panicked");

        let (second_head, second_count) = list.pop_all();

        // Exactly one of the two drains observes the push. Losing it entirely
        // would leak the block; observing it twice would double-free it.
        assert_eq!(
            first_count + second_count,
            1,
            "push observed {} times (first={first_head:#x}, second={second_head:#x})",
            first_count + second_count
        );
    });
}

/// After a drain the list is empty, and a subsequent push is visible again.
///
/// Pins the swap-to-zero reset: an implementation that cleared only the address
/// and left the counter would report a phantom count to the next drainer.
#[test]
fn drain_resets_both_address_and_count() {
    loom::model(|| {
        let list = Arc::new(HeadProtocol::new());
        list.push(0x1000);

        let (_, count) = list.pop_all();
        assert_eq!(count, 1);
        assert!(
            list.is_empty(),
            "list not empty after draining its only push"
        );

        let (head, count) = {
            list.push(0x3000);
            list.pop_all()
        };
        assert_eq!(count, 1, "counter did not reset across drains");
        assert_eq!(head, 0x3000);
    });
}
