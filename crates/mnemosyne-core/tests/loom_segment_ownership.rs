//! Loom models for `SegmentOwnership`, the segment's owner identity pair.
//!
//! A segment carries who owns it and which allocator cache its frees route to.
//! The two are only meaningful together: a thread that observes an owner and
//! then reads an allocator belonging to the *previous* owner has read a torn
//! identity, and will hand a freed block to a cache that does not own the
//! segment. The pair is published under `Release` and observed under `Acquire`
//! precisely so that cannot happen.
//!
//! Unlike the free-list models, these drive the shipped type directly rather
//! than a reduction of it — `SegmentOwnership` was split out of `Segment` for
//! exactly this reason. A model cannot build a whole `Segment`, whose
//! `[Page; PAGES_PER_SEGMENT]` would create one instrumented atomic per page,
//! but it can build the pair.
//!
//! Run with:
//!
//! ```text
//! RUSTFLAGS="--cfg loom" cargo test -p mnemosyne-memory-core --test loom_segment_ownership --release
//! ```
//!
//! The whole file is `cfg(loom)`-gated: without that flag the shim yields real
//! atomics and `loom::model` does not exist.
#![cfg(loom)]

use loom::sync::Arc;
use mnemosyne_core::types::{SegmentOwner, SegmentOwnership};

/// Stand-in allocator-cache addresses. Their values are irrelevant; only the
/// pairing with an owner matters.
const ALLOC_A: usize = 0x1000;
const ALLOC_B: usize = 0x2000;

fn alloc_ptr(addr: usize) -> *mut core::ffi::c_void {
    addr as *mut core::ffi::c_void
}

fn owner_for(addr: usize) -> SegmentOwner {
    SegmentOwner::from_ptr(alloc_ptr(addr))
}

/// Publishing an owner publishes everything that owner wrote first.
///
/// This is the edge the protocol actually rests on. A segment is initialized —
/// pages, keys, free-list state — and *then* its owner is published under
/// `Release`. Any thread that observes that owner under `Acquire` must see the
/// initialization, or it would operate on a half-built segment.
///
/// Modelled the way the code does it: a payload written before `set_owner`,
/// and a reader that only touches the payload once it has seen the owner.
#[test]
fn observing_an_owner_implies_seeing_what_it_published_first() {
    loom::model(|| {
        // Stands in for the segment state a claim publishes: page metadata,
        // key schedule, free-list heads.
        let contents = Arc::new(loom::cell::UnsafeCell::new(0usize));
        let pair = Arc::new(SegmentOwnership::unowned());

        let claimer = {
            let (pair, contents) = (Arc::clone(&pair), Arc::clone(&contents));
            loom::thread::spawn(move || {
                contents.with_mut(|c| unsafe { *c = 0xABCD });
                pair.set_owner(owner_for(ALLOC_A));
            })
        };

        let reader = {
            let (pair, contents) = (Arc::clone(&pair), Arc::clone(&contents));
            loom::thread::spawn(move || {
                if pair.owner().matches(alloc_ptr(ALLOC_A)) {
                    let seen = contents.with(|c| unsafe { *c });
                    assert_eq!(
                        seen, 0xABCD,
                        "observed the owner but not the state it published                          before claiming: the Release/Acquire edge is not holding"
                    );
                }
            })
        };

        claimer.join().expect("claimer panicked");
        reader.join().expect("reader panicked");
    });
}

/// Orphaning publishes the teardown that preceded it.
///
/// The mirror of the claim edge. A thread that observes `NONE` must see the
/// writes the departing owner made before releasing the segment, since the next
/// claimant reads them.
#[test]
fn observing_an_orphan_implies_seeing_the_teardown() {
    loom::model(|| {
        let contents = Arc::new(loom::cell::UnsafeCell::new(0usize));
        let pair = Arc::new(SegmentOwnership::unowned());
        pair.set_owner(owner_for(ALLOC_A));
        pair.set_allocator(alloc_ptr(ALLOC_A));

        let orphaner = {
            let (pair, contents) = (Arc::clone(&pair), Arc::clone(&contents));
            loom::thread::spawn(move || {
                contents.with_mut(|c| unsafe { *c = 0xDEAD });
                pair.set_owner(SegmentOwner::NONE);
            })
        };

        let reader = {
            let (pair, contents) = (Arc::clone(&pair), Arc::clone(&contents));
            loom::thread::spawn(move || {
                if pair.owner().0 == SegmentOwner::NONE.0 {
                    let seen = contents.with(|c| unsafe { *c });
                    assert_eq!(
                        seen, 0xDEAD,
                        "observed the segment as unowned without seeing the                          teardown that released it"
                    );
                }
            })
        };

        orphaner.join().expect("orphaner panicked");
        reader.join().expect("reader panicked");
    });
}

/// The pair settles consistently once both writers are done.
///
/// The owner and the allocator are two stores, so a reader *between* them sees
/// a mixed pair — that is inherent to updating two locations, and no ordering
/// fixes it. The free path is unharmed because it only reads the allocator
/// after matching the owner against its own token, and a thread that matches is
/// the one that wrote both. What must hold is that the pair converges: once the
/// claim completes, owner and allocator agree.
#[test]
fn the_pair_agrees_once_a_claim_completes() {
    loom::model(|| {
        let pair = Arc::new(SegmentOwnership::unowned());

        let claimer = {
            let pair = Arc::clone(&pair);
            loom::thread::spawn(move || {
                pair.set_owner(owner_for(ALLOC_B));
                pair.set_allocator(alloc_ptr(ALLOC_B));
            })
        };
        // A concurrent observer, so the claim is genuinely raced rather than
        // running alone.
        let observer = {
            let pair = Arc::clone(&pair);
            loom::thread::spawn(move || {
                let _ = pair.owner();
                let _ = pair.allocator();
            })
        };

        claimer.join().expect("claimer panicked");
        observer.join().expect("observer panicked");

        assert!(pair.owner().matches(alloc_ptr(ALLOC_B)));
        assert_eq!(pair.allocator() as usize, ALLOC_B);
    });
}

/// Whatever a reader observes is a state the protocol actually published.
///
/// Neither an owner nor an allocator may appear from nowhere: the only values
/// either side can hold are the ones written by the two writers plus the
/// initial state. This is the invariant that a mis-typed or mis-ordered store
/// breaks first, and it holds regardless of which interleaving loom picks.
#[test]
fn observed_values_are_always_ones_that_were_published() {
    loom::model(|| {
        let pair = Arc::new(SegmentOwnership::unowned());

        let writer = {
            let pair = Arc::clone(&pair);
            loom::thread::spawn(move || {
                pair.set_owner(owner_for(ALLOC_A));
                pair.set_allocator(alloc_ptr(ALLOC_A));
            })
        };

        let reader = {
            let pair = Arc::clone(&pair);
            loom::thread::spawn(move || (pair.owner(), pair.allocator() as usize))
        };

        writer.join().expect("writer panicked");
        let (owner, allocator) = reader.join().expect("reader panicked");

        assert!(
            owner.matches(alloc_ptr(ALLOC_A)) || owner.0 == SegmentOwner::NONE.0,
            "observed an owner that was never published"
        );
        assert!(
            allocator == ALLOC_A || allocator == 0,
            "observed an allocator that was never published"
        );
    });
}
