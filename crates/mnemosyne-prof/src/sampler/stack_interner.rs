use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::hasher::FastBuildHasher;

/// Interned identity of a captured stack trace.
///
/// Samples store this `u32` handle instead of an owned `Box<[usize]>`, so the
/// per-live-allocation metadata is a fixed 4 bytes regardless of stack depth and
/// the actual frame arrays are deduplicated: the leak detector's retained memory
/// scales with the number of *distinct call sites*, not the number of live
/// allocations (which can differ by orders of magnitude).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct StackId(u32);

const STACK_INTERNER_SHARDS: usize = 64;
const STACK_INTERNER_SHARD_BITS: u32 = STACK_INTERNER_SHARDS.trailing_zeros();
const STACK_ID_LOCAL_BITS: u32 = u32::BITS - STACK_INTERNER_SHARD_BITS;
const STACK_ID_LOCAL_MASK: u32 = (1u32 << STACK_ID_LOCAL_BITS) - 1;
const _: () = assert!(STACK_INTERNER_SHARDS.is_power_of_two());

impl StackId {
    #[inline]
    fn new(shard: usize, local_id: u32) -> Self {
        debug_assert!(shard < STACK_INTERNER_SHARDS);
        debug_assert!(local_id <= STACK_ID_LOCAL_MASK);
        Self(((shard as u32) << STACK_ID_LOCAL_BITS) | local_id)
    }

    #[inline]
    fn shard(self) -> usize {
        (self.0 >> STACK_ID_LOCAL_BITS) as usize
    }

    #[inline]
    fn local_index(self) -> usize {
        (self.0 & STACK_ID_LOCAL_MASK) as usize
    }

    #[inline]
    fn local_id(self) -> u32 {
        self.0 & STACK_ID_LOCAL_MASK
    }
}

/// Global stack-trace interner shared by every sampled allocation.
///
/// Each distinct live frame sequence is stored exactly once as an `Arc<[usize]>`.
/// Repeat call sites increment a reference count without allocating; the last
/// free removes the content-keyed entry and recycles the id slot.
struct StackInternerShard {
    forward: HashMap<Arc<[usize]>, StackId, FastBuildHasher>,
    entries: Vec<Option<StackEntry>>,
    free_ids: Vec<u32>,
}

struct StackEntry {
    frames: Arc<[usize]>,
    refs: usize,
}

type RetiredStack = (Arc<[usize]>, Arc<[usize]>);

#[repr(align(64))]
struct InternerShard {
    mutex: Mutex<Option<StackInternerShard>>,
}

static STACK_INTERNER: [InternerShard; STACK_INTERNER_SHARDS] = [const {
    InternerShard {
        mutex: Mutex::new(None),
    }
}; STACK_INTERNER_SHARDS];

fn stack_interner_shard(frames: &[usize]) -> usize {
    let mut hasher = <FastBuildHasher as std::hash::BuildHasher>::build_hasher(&FastBuildHasher);
    std::hash::Hash::hash(&frames, &mut hasher);
    (std::hash::Hasher::finish(&hasher) as usize) & (STACK_INTERNER_SHARDS - 1)
}

fn get_stack_interner(shard: usize) -> std::sync::MutexGuard<'static, Option<StackInternerShard>> {
    let mut lock = STACK_INTERNER[shard]
        .mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if lock.is_none() {
        *lock = Some(StackInternerShard {
            forward: HashMap::with_hasher(FastBuildHasher),
            entries: Vec::new(),
            free_ids: Vec::new(),
        });
    }
    lock
}

pub(super) fn resolve_stack(id: StackId) -> Option<Arc<[usize]>> {
    let guard = STACK_INTERNER[id.shard()]
        .mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.as_ref().and_then(|interner| interner.resolve(id))
}

/// Interns `frames`, returning its stable [`StackId`]. Allocates only on a
/// first-seen call site; repeat sites are a hash lookup with no allocation.
pub(super) fn intern_stack(frames: &[usize]) -> StackId {
    let shard = stack_interner_shard(frames);
    {
        let mut guard = get_stack_interner(shard);
        let interner = guard
            .as_mut()
            .expect("stack interner shard must be initialized");
        if let Some(&id) = interner.forward.get(frames) {
            return interner.retain(id);
        }
    }

    let arc: Arc<[usize]> = Arc::from(frames);
    let mut guard = get_stack_interner(shard);
    let interner = guard
        .as_mut()
        .expect("stack interner shard must be initialized");
    if let Some(&id) = interner.forward.get(arc.as_ref()) {
        return interner.retain(id);
    }
    let id = if let Some(local_id) = interner.free_ids.pop() {
        let id = StackId::new(shard, local_id);
        interner.entries[local_id as usize] = Some(StackEntry {
            frames: Arc::clone(&arc),
            refs: 1,
        });
        id
    } else {
        assert!(
            interner.entries.len() <= STACK_ID_LOCAL_MASK as usize,
            "invariant: stack interner shard id count exceeds its bit budget"
        );
        let local_id = u32::try_from(interner.entries.len())
            .expect("invariant: stack interner shard id count exceeds u32::MAX");
        let id = StackId::new(shard, local_id);
        interner.entries.push(Some(StackEntry {
            frames: Arc::clone(&arc),
            refs: 1,
        }));
        id
    };
    interner.forward.insert(arc, id);
    id
}

pub(super) fn release_stack(id: StackId) {
    let mut guard = STACK_INTERNER[id.shard()]
        .mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let retired = guard.as_mut().and_then(|interner| interner.release(id));
    drop(guard);
    // The entry and map key can hold the final two strong references. Their
    // backing allocation is released after the shard lock, so allocator work
    // cannot lengthen the interner's critical section or re-enter it.
    drop(retired);
}

pub(super) fn reset_stack_interner_state() {
    for shard in &STACK_INTERNER {
        let mut lock = shard
            .mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *lock = None;
    }
}

impl StackInternerShard {
    fn retain(&mut self, id: StackId) -> StackId {
        let entry = self
            .entries
            .get_mut(id.local_index())
            .and_then(Option::as_mut)
            .expect("invariant: stack interner forward map points at a live entry");
        entry.refs = entry
            .refs
            .checked_add(1)
            .expect("invariant: stack interner reference count overflow");
        id
    }

    fn resolve(&self, id: StackId) -> Option<Arc<[usize]>> {
        self.entries
            .get(id.local_index())
            .and_then(Option::as_ref)
            .map(|entry| Arc::clone(&entry.frames))
    }

    fn release(&mut self, id: StackId) -> Option<RetiredStack> {
        let entry_slot = self.entries.get_mut(id.local_index())?;
        if entry_slot.as_ref()?.refs > 1 {
            entry_slot.as_mut()?.refs -= 1;
            return None;
        }

        let entry = entry_slot
            .take()
            .expect("invariant: checked live stack entry must remain present");
        let (key, removed_id) = self
            .forward
            .remove_entry(entry.frames.as_ref())
            .expect("invariant: live stack entry must have a forward-map key");
        assert_eq!(
            removed_id, id,
            "invariant: stack interner forward map points at a different id"
        );
        self.free_ids.push(id.local_id());
        Some((entry.frames, key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames_for_shards<const N: usize>() -> [(usize, [usize; 2]); N] {
        let mut frames = [(usize::MAX, [0usize; 2]); N];
        let mut found = 0usize;
        for word in 1..16_384usize {
            let stack = [0x7ff6_0000_0000usize | word, 0x7ff6_ffff_ffffusize];
            let shard = stack_interner_shard(&stack);
            if frames[..found].iter().all(|(seen, _)| *seen != shard) {
                frames[found] = (shard, stack);
                found += 1;
                if found == N {
                    return frames;
                }
            }
        }
        panic!("invariant: deterministic stack hash did not cover {N} distinct shards");
    }

    fn distinct_frames_for_shard(shard: usize, excluded: &[usize]) -> [usize; 2] {
        for word in 1..16_384usize {
            let stack = [0x7ff6_1000_0000usize | word, 0x7ff6_ffff_ffffusize];
            if stack_interner_shard(&stack) == shard && stack.as_slice() != excluded {
                return stack;
            }
        }
        panic!("invariant: deterministic stack hash did not find a distinct same-shard stack");
    }

    #[test]
    fn stack_interner_hash_covers_all_shards() {
        let frames = frames_for_shards::<STACK_INTERNER_SHARDS>();
        let mut seen = [false; STACK_INTERNER_SHARDS];
        for (shard, stack) in frames {
            assert_eq!(
                stack_interner_shard(&stack),
                shard,
                "fixture must route to its recorded shard"
            );
            seen[shard] = true;
        }
        assert!(
            seen.into_iter().all(|covered| covered),
            "deterministic stack fixtures must cover every interner shard"
        );
    }

    #[test]
    fn stack_interner_encodes_shard_and_local_id() {
        crate::reset_profiler_for_testing();

        let [(first_shard, first_stack), (second_shard, second_stack)] = frames_for_shards::<2>();
        let first = intern_stack(&first_stack);
        let second = intern_stack(&second_stack);

        assert_eq!(first.shard(), first_shard);
        assert_eq!(second.shard(), second_shard);
        assert_eq!(first.local_id(), 0);
        assert_eq!(second.local_id(), 0);
        assert_ne!(
            first, second,
            "equal local ids in distinct shards must still form distinct StackIds"
        );

        release_stack(first);
        release_stack(second);
        crate::reset_profiler_for_testing();
    }

    #[test]
    fn stack_interner_reuses_ids_and_releases_last_reference() {
        crate::reset_profiler_for_testing();

        let first = intern_stack(&[1, 2, 3]);
        let repeat = intern_stack(&[1, 2, 3]);
        assert_eq!(first, repeat);

        {
            let guard = STACK_INTERNER[first.shard()]
                .mutex
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let interner = guard.as_ref().expect("stack interner must be initialized");
            let entry = interner.entries[first.local_index()]
                .as_ref()
                .expect("interned stack id must point to a live entry");
            assert_eq!(entry.refs, 2);
            assert_eq!(interner.forward.len(), 1);
        }

        release_stack(first);
        {
            let guard = STACK_INTERNER[first.shard()]
                .mutex
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let interner = guard
                .as_ref()
                .expect("stack interner must stay initialized");
            let entry = interner.entries[first.local_index()]
                .as_ref()
                .expect("one remaining reference must keep the entry live");
            assert_eq!(entry.refs, 1);
            assert_eq!(interner.forward.len(), 1);
        }

        release_stack(repeat);
        {
            let guard = STACK_INTERNER[first.shard()]
                .mutex
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let interner = guard
                .as_ref()
                .expect("stack interner must stay initialized");
            assert!(interner.entries[first.local_index()].is_none());
            assert!(interner.forward.is_empty());
            assert_eq!(interner.free_ids.as_slice(), &[first.local_id()]);
        }

        let same_shard = distinct_frames_for_shard(first.shard(), &[1, 2, 3]);
        let reused = intern_stack(&same_shard);
        assert_eq!(
            reused, first,
            "released same-shard stack ids should be recycled instead of growing the table"
        );

        crate::reset_profiler_for_testing();
    }

    #[test]
    fn stack_interner_interns_distinct_shards_concurrently() {
        crate::reset_profiler_for_testing();

        let frames = frames_for_shards::<STACK_INTERNER_SHARDS>();
        let barrier = Arc::new(std::sync::Barrier::new(STACK_INTERNER_SHARDS));
        let mut workers = Vec::with_capacity(STACK_INTERNER_SHARDS);
        for (expected_shard, stack) in frames {
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                let id = intern_stack(&stack);
                assert_eq!(id.shard(), expected_shard);
                id
            }));
        }

        let ids: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().expect("interner worker must not panic"))
            .collect();
        assert_eq!(ids.len(), STACK_INTERNER_SHARDS);
        for id in &ids {
            let guard = STACK_INTERNER[id.shard()]
                .mutex
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let interner = guard.as_ref().expect("stack interner must be initialized");
            let entry = interner.entries[id.local_index()]
                .as_ref()
                .expect("worker interned stack id must point to a live entry");
            assert_eq!(entry.refs, 1);
        }
        for id in ids {
            release_stack(id);
        }

        crate::reset_profiler_for_testing();
    }
}
