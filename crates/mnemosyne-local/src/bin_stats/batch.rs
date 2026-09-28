use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use mnemosyne_core::constants::NUM_SIZE_CLASSES;

// Two process-wide per-class atomic arrays. Allocation bytes are derived from
// the immutable class stride when a snapshot is read.
pub(super) static ALLOC_COUNT: [AtomicU64; NUM_SIZE_CLASSES] =
    [const { AtomicU64::new(0) }; NUM_SIZE_CLASSES];
pub(super) static DEALLOC_COUNT: [AtomicU64; NUM_SIZE_CLASSES] =
    [const { AtomicU64::new(0) }; NUM_SIZE_CLASSES];
/// Cumulative user-requested bytes per class; used to compute internal
/// fragmentation: `(alloc_bytes - requested_bytes) / alloc_bytes`.
///
/// Updated with a direct relaxed `fetch_add` (not batched) because the
/// request size varies per call and cannot be accumulated in the
/// fixed-class `PendingCount` slots. The hot-path overhead is one extra
/// `LOCK XADD` per allocation, which is dominated by the cache-line cost
/// of the alloc itself.
pub(super) static REQUESTED_BYTES: [AtomicU64; NUM_SIZE_CLASSES] =
    [const { AtomicU64::new(0) }; NUM_SIZE_CLASSES];

/// Process-wide reset generation counter. Incremented on every `reset_bin_stats()`.
///
/// Each TLS batch records the generation it was started in. When a flush
/// observes that this counter has advanced, the batch is discarded rather
/// than adding stale pre-reset counts to the fresh global arrays.
pub(super) static RESET_GENERATION: AtomicU32 = AtomicU32::new(0);

// Eight direct-mapped entries cover the common case of one or a few active
// size classes while keeping the per-thread footprint bounded at 256 bytes.
// A class collision flushes the displaced entry; it never drops observations.
const PENDING_SLOTS: usize = 8;
pub(super) const FLUSH_BATCH: u32 = 64;
const EMPTY_CLASS: usize = usize::MAX;

const _: () = assert!(PENDING_SLOTS.is_power_of_two());

#[derive(Clone, Copy)]
pub(super) struct PendingCount {
    pub(super) class: usize,
    pub(super) count: u32,
    /// The `RESET_GENERATION` value when this slot was first populated.
    /// If the global generation has since advanced, this batch is stale
    /// and will be discarded rather than flushed.
    generation: u32,
}

impl PendingCount {
    pub(super) const fn new() -> Self {
        Self {
            class: EMPTY_CLASS,
            count: 0,
            generation: 0,
        }
    }

    #[inline(always)]
    pub(super) fn record(&mut self, class: usize, global: &[AtomicU64; NUM_SIZE_CLASSES]) {
        if self.class != class {
            self.flush(global);
            self.class = class;
            // Stamp the generation when starting a new accumulation slot.
            self.generation = RESET_GENERATION.load(Ordering::Relaxed);
        }

        self.count += 1;
        if self.count == FLUSH_BATCH {
            self.flush(global);
        }
    }

    #[inline]
    pub(super) fn flush(&mut self, global: &[AtomicU64; NUM_SIZE_CLASSES]) {
        if self.count != 0 {
            // If the global reset generation has advanced past the one
            // recorded when we started accumulating, discard the stale batch.
            let current_gen = RESET_GENERATION.load(Ordering::Relaxed);
            if current_gen == self.generation && self.class < NUM_SIZE_CLASSES {
                global[self.class].fetch_add(self.count as u64, Ordering::Relaxed);
            }
            // Always reset regardless of whether we flushed.
            self.count = 0;
            self.class = EMPTY_CLASS;
        }
    }
}

struct ThreadBinStats {
    alloc: [PendingCount; PENDING_SLOTS],
    dealloc: [PendingCount; PENDING_SLOTS],
}

impl ThreadBinStats {
    const fn new() -> Self {
        Self {
            alloc: [PendingCount::new(); PENDING_SLOTS],
            dealloc: [PendingCount::new(); PENDING_SLOTS],
        }
    }

    #[inline(always)]
    fn record_alloc(&mut self, class: usize) {
        self.alloc[class & (PENDING_SLOTS - 1)].record(class, &ALLOC_COUNT);
    }

    #[inline(always)]
    fn record_dealloc(&mut self, class: usize) {
        self.dealloc[class & (PENDING_SLOTS - 1)].record(class, &DEALLOC_COUNT);
    }

    #[inline]
    fn flush(&mut self) {
        for pending in &mut self.alloc {
            pending.flush(&ALLOC_COUNT);
        }
        for pending in &mut self.dealloc {
            pending.flush(&DEALLOC_COUNT);
        }
    }
}

impl Drop for ThreadBinStats {
    fn drop(&mut self) {
        self.flush();
    }
}

std::thread_local! {
    static THREAD_STATS: core::cell::UnsafeCell<ThreadBinStats> =
        const { core::cell::UnsafeCell::new(ThreadBinStats::new()) };
}

#[inline]
pub(super) fn allocation_bytes(alloc_count: u64, block_size: usize) -> u64 {
    alloc_count.saturating_mul(block_size as u64)
}

/// Records one allocation with the explicit adjusted request size.
///
/// Updates both the batched alloc-count and the direct requested-bytes
/// counter so per-class internal fragmentation can be measured.
#[inline(always)]
pub(crate) fn record_alloc_with_size(class: usize, adjusted_size: usize) {
    if class < NUM_SIZE_CLASSES {
        THREAD_STATS.with(|stats| {
            // SAFETY: `THREAD_STATS` is owned by the current thread.
            unsafe { (*stats.get()).record_alloc(class) };
        });
        REQUESTED_BYTES[class].fetch_add(adjusted_size as u64, Ordering::Relaxed);
    }
}

/// Records one deallocation into `class`.
#[inline(always)]
pub(crate) fn record_dealloc(class: usize) {
    if class < NUM_SIZE_CLASSES {
        THREAD_STATS.with(|stats| {
            // SAFETY: `THREAD_STATS` is owned by the current thread. The
            // closure cannot run concurrently for the same TLS value.
            unsafe { (*stats.get()).record_dealloc(class) };
        });
    }
}

#[inline]
pub(super) fn flush_current_thread() {
    THREAD_STATS.with(|stats| {
        // SAFETY: `THREAD_STATS` is owned by the current thread. The closure
        // cannot run concurrently for the same TLS value.
        unsafe { (*stats.get()).flush() };
    });
}
