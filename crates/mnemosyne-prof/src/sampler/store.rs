use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::hasher::FastBuildHasher;
use super::stack_interner::{StackId, resolve_stack};

/// Representation of a sampled memory allocation.
#[derive(Clone, Copy)]
pub struct Sample {
    /// Allocated size of the block in bytes.
    pub size: usize,
    /// Interned identity of the retained stack trace (resolve via the interner).
    pub stack: StackId,
}

#[derive(Clone)]
pub(super) struct ActiveSample {
    pub(super) ptr: usize,
    pub(super) size: usize,
    pub(super) stack: Arc<[usize]>,
}

const SHARDS: usize = 64;

#[repr(align(64))]
struct Shard {
    mutex: Mutex<Option<HashMap<usize, Sample, FastBuildHasher>>>,
    active: AtomicBool,
}

static ACTIVE_SAMPLES: [Shard; SHARDS] = [const {
    Shard {
        mutex: Mutex::new(None),
        active: AtomicBool::new(false),
    }
}; SHARDS];

fn get_map(
    shard: usize,
) -> std::sync::MutexGuard<'static, Option<HashMap<usize, Sample, FastBuildHasher>>> {
    let mut lock = ACTIVE_SAMPLES[shard]
        .mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if lock.is_none() {
        *lock = Some(HashMap::with_hasher(FastBuildHasher));
    }
    lock
}

pub(super) fn reset_active_samples() {
    for shard in &ACTIVE_SAMPLES {
        let mut lock = shard
            .mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *lock = None;
        shard.active.store(false, Ordering::Release);
    }
}

pub(super) fn insert_sample(ptr: usize, sample: Sample) -> Option<Sample> {
    let shard = sample_shard(ptr);
    let mut lock = get_map(shard);
    let replaced = if let Some(ref mut map) = *lock {
        map.insert(ptr, sample)
    } else {
        None
    };
    if replaced.is_none() {
        ACTIVE_SAMPLES[shard].active.store(true, Ordering::Release);
    }
    replaced
}

pub(super) fn remove_sample(ptr: usize) -> Option<Sample> {
    let shard = sample_shard(ptr);
    let mut lock = ACTIVE_SAMPLES[shard]
        .mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (removed, became_empty) = match lock.as_mut() {
        Some(map) => {
            let removed = map.remove(&ptr);
            let became_empty = removed.is_some() && map.is_empty();
            (removed, became_empty)
        }
        None => return None,
    };
    if became_empty {
        ACTIVE_SAMPLES[shard].active.store(false, Ordering::Release);
    }
    removed
}

pub(super) fn active_sample_snapshot() -> Vec<ActiveSample> {
    let mut samples = Vec::new();
    for shard in &ACTIVE_SAMPLES {
        let lock = shard
            .mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(ref map) = *lock {
            samples.extend(map.iter().filter_map(|(&ptr, sample)| {
                resolve_stack(sample.stack).map(|stack| ActiveSample {
                    ptr,
                    size: sample.size,
                    stack,
                })
            }));
        }
    }
    samples
}

#[inline]
pub(crate) fn has_active_sample_for(ptr: usize) -> bool {
    ACTIVE_SAMPLES[sample_shard(ptr)]
        .active
        .load(Ordering::Acquire)
}

#[inline]
fn sample_shard(ptr: usize) -> usize {
    (ptr >> 6) % SHARDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_sample_snapshot_is_detached_from_live_shards() {
        crate::reset_profiler_for_testing();

        let ptr = 0x1000usize as *mut u8;
        crate::sampler::sample_alloc_inner(ptr, 64, true);

        let snapshot = active_sample_snapshot();
        assert_eq!(snapshot.len(), 1);
        assert!(has_active_sample_for(ptr as usize));
        assert_eq!(snapshot[0].ptr, ptr as usize);
        assert_eq!(snapshot[0].size, 64);
        assert!(
            !snapshot[0].stack.is_empty(),
            "snapshot must retain resolved stack frames"
        );

        crate::sampler::sample_free_inner(ptr);
        assert!(
            active_sample_snapshot().is_empty(),
            "live shard map must be empty after freeing the sampled pointer"
        );
        assert!(!has_active_sample_for(ptr as usize));
        assert_eq!(
            snapshot[0].size, 64,
            "snapshot must retain a value copy after the live sample is removed"
        );

        crate::reset_profiler_for_testing();
    }
}
