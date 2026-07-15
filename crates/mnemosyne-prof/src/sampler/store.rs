use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::ACTIVE_SAMPLES_COUNT;

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
}

static ACTIVE_SAMPLES: [Shard; SHARDS] = [const {
    Shard {
        mutex: Mutex::new(None),
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
    }
}

pub(super) fn insert_sample(ptr: usize, sample: Sample) -> Option<Sample> {
    let mut lock = get_map(sample_shard(ptr));
    let replaced = if let Some(ref mut map) = *lock {
        map.insert(ptr, sample)
    } else {
        None
    };
    if replaced.is_none() {
        ACTIVE_SAMPLES_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }
    replaced
}

pub(super) fn remove_sample(ptr: usize) -> Option<Sample> {
    let mut lock = get_map(sample_shard(ptr));
    let removed = if let Some(ref mut map) = *lock {
        map.remove(&ptr)
    } else {
        None
    };
    if removed.is_some() {
        ACTIVE_SAMPLES_COUNT.fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
    }
    removed
}

pub(super) fn active_sample_snapshot() -> Vec<ActiveSample> {
    let total = ACTIVE_SAMPLES_COUNT.load(core::sync::atomic::Ordering::Relaxed);
    let mut samples = Vec::with_capacity(total);
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
fn sample_shard(ptr: usize) -> usize {
    (ptr >> 6) % SHARDS
}
