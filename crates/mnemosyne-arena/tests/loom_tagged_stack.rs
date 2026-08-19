//! Loom models for `TaggedSegmentStack`'s pop protocol.
//!
//! MN-433 kept this the lowest-priority model on the grounds that the stack
//! serializes pushes on a mutation lock and already *states* its reclamation and
//! Acquire-on-CAS-failure arguments, so a model would confirm a written
//! argument rather than probe an unexamined one. That is a reason to do it last,
//! not a reason to skip it: a written argument is exactly the kind that stays
//! convincing after the code stops matching it.
//!
//! What is modelled is the pop side, because that is the lock-free part. Pushes
//! hold `mutation_lock`, so at most one splice runs at a time and push-versus-
//! push is not reachable; a single pusher is therefore a faithful stand-in for
//! any number of them.
//!
//! # Fidelity
//!
//! A reduction, for the same reason as the free-list models: the shipped stack
//! links real `Segment`s through `next_free_segment`, and a `Segment` embeds
//! `[Page; PAGES_PER_SEGMENT]`, so building one under loom creates an
//! instrumented atomic per page. Nodes here are indices into a fixed array and
//! their links live in `loom::cell::UnsafeCell`, while the orderings, the tag
//! packing, and the CAS loops are exactly the shipped ones — in particular the
//! `Acquire` *failure* ordering on pop, which is the argument under test.
//!
//! Run with:
//!
//! ```text
//! RUSTFLAGS="--cfg loom" cargo test -p mnemosyne-arena --test loom_tagged_stack --release
//! ```
#![cfg(loom)]

use loom::sync::Arc;
use loom::sync::atomic::{AtomicUsize, Ordering};

/// Node handles are 1-based so that zero can mean "empty", exactly as a null
/// `*mut Segment` does in the shipped stack.
const EMPTY: usize = 0;
const NODES: usize = 2;

const PTR_BITS: u32 = 48;
const PTR_MASK: usize = (1usize << PTR_BITS) - 1;

fn ptr_of(head: usize) -> usize {
    head & PTR_MASK
}

/// Mirrors `TaggedHead::tagged_successor`: the successor address carries a tag
/// one greater than the head it replaces, so a pop cannot succeed against a
/// head that was recycled to the same address in between.
fn tagged_successor(next: usize, from: usize) -> usize {
    let tag = (from >> PTR_BITS).wrapping_add(1);
    (tag << PTR_BITS) | (next & PTR_MASK)
}

/// A reduction of the stack: a tagged head plus per-node links.
struct TaggedStack {
    head: AtomicUsize,
    /// `links[i]` is node `i + 1`'s `next_free_segment`, atomic for the same
    /// reason the shipped field is: a popper clearing it races a losing
    /// popper's read of it.
    links: [AtomicUsize; NODES],
}

impl TaggedStack {
    fn new() -> Self {
        Self {
            head: AtomicUsize::new(EMPTY),
            links: [AtomicUsize::new(EMPTY), AtomicUsize::new(EMPTY)],
        }
    }

    fn set_link(&self, node: usize, next: usize) {
        self.links[node - 1].store(next, Ordering::Relaxed);
    }

    fn link(&self, node: usize) -> usize {
        self.links[node - 1].load(Ordering::Relaxed)
    }

    /// Mirrors `splice_locked`: link the node to the observed head, then publish
    /// with a `Release` CAS and a `Relaxed` failure ordering. The failure value
    /// is only re-linked into a node this thread exclusively owns, never
    /// dereferenced, which is why `Relaxed` is sound here and not on pop.
    fn push(&self, node: usize) {
        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            self.set_link(node, ptr_of(current));
            let next = tagged_successor(node, current);
            match self.head.compare_exchange_weak(
                current,
                next,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }

    /// Mirrors `pop`: `Acquire` on the initial load *and* on CAS failure,
    /// because the failure value is dereferenced on the next iteration.
    fn pop(&self) -> usize {
        let mut current = self.head.load(Ordering::Acquire);
        loop {
            let node = ptr_of(current);
            if node == EMPTY {
                return EMPTY;
            }
            let next = tagged_successor(self.link(node), current);
            match self.head.compare_exchange_weak(
                current,
                next,
                Ordering::Acquire,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    // Mirrors the shipped `pop`, which clears the link here.
                    // The clearing is kept — three tests pin it — and the race
                    // it used to create is gone because the link is atomic.
                    self.set_link(node, EMPTY);
                    return node;
                }
                Err(actual) => current = actual,
            }
        }
    }
}

/// A popper sees the link the pusher wrote before publishing.
///
/// This is what the `Acquire` on pop's initial load and CAS failure buys. The
/// pusher writes the node's successor and *then* publishes it with a `Release`
/// CAS; a popper that observes the node must observe that link, or it would
/// install a garbage successor as the new head and hand out a segment that is
/// not on the stack.
#[test]
fn a_popped_node_carries_the_link_its_pusher_published() {
    loom::model(|| {
        let stack = Arc::new(TaggedStack::new());
        // Node 1 is already on the stack; node 2 is pushed concurrently, so a
        // popper can observe either and a torn link has a wrong value available.
        stack.push(1);

        let pusher = {
            let stack = Arc::clone(&stack);
            loom::thread::spawn(move || stack.push(2))
        };
        let popped = {
            let stack = Arc::clone(&stack);
            loom::thread::spawn(move || {
                let node = stack.pop();
                (node, stack.head.load(Ordering::Acquire))
            })
            .join()
            .expect("popper panicked")
        };
        pusher.join().expect("pusher panicked");

        let (node, head_after) = popped;
        if node == 2 {
            // Popping node 2 must have installed node 1 as the head — the link
            // the pusher wrote — never `EMPTY` or a stale successor.
            assert_eq!(
                ptr_of(head_after),
                1,
                "popped the newly pushed node but installed the wrong successor: \
                 the publishing Release/Acquire edge did not hold"
            );
        }
    });
}

/// No node is handed to two poppers.
///
/// A segment popped twice is a segment two threads both believe they own, which
/// is the failure the tag exists to prevent: without it a stale head could
/// still CAS successfully after the address was recycled.
/// This is the model that found MN-455. It failed with loom reporting
/// "Causality violation: Concurrent read and write accesses" between the
/// winning popper's link-clearing write and a losing popper's read of the same
/// field, and it passes now that the link is atomic.
#[test]
fn concurrent_pops_never_hand_out_the_same_node() {
    loom::model(|| {
        let stack = Arc::new(TaggedStack::new());
        stack.push(1);
        stack.push(2);

        let a = {
            let stack = Arc::clone(&stack);
            loom::thread::spawn(move || stack.pop())
        };
        let b = {
            let stack = Arc::clone(&stack);
            loom::thread::spawn(move || stack.pop())
        };
        let (a, b) = (
            a.join().expect("popper a panicked"),
            b.join().expect("popper b panicked"),
        );

        assert!(
            a == EMPTY || b == EMPTY || a != b,
            "both poppers were handed node {a}: two threads now own one segment"
        );
        // Between them they took the two nodes and left the stack empty.
        assert_eq!(
            ptr_of(stack.head.load(Ordering::Acquire)),
            EMPTY,
            "two successful pops left something on a two-node stack"
        );
    });
}
