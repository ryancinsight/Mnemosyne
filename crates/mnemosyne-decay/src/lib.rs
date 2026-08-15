//! Background decay and reclamation for Mnemosyne arenas.
//!
//! Segments freed by the allocator are not returned to the operating
//! system immediately: holding them lets a subsequent allocation of the
//! same size class reuse the mapping instead of paying another syscall.
//! This crate owns the opposite side of that trade — it periodically
//! purges segments that have gone cold, so a burst of allocation does not
//! pin resident memory indefinitely.
//!
//! [`init_decay_engine`] lazily spawns the worker thread, gated on the
//! `MNEMOSYNE_PURGE_CADENCE_MS` cadence (zero disables decay entirely and
//! spawns no thread). [`decay_step`] performs one sweep across the active
//! backends and is public so a caller running its own scheduler, or a test
//! needing determinism, can drive reclamation without the background
//! thread.

#![deny(missing_docs)]

use core::sync::atomic::Ordering;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::options::PURGE_CADENCE_MS;
use mnemosyne_core::types::{Page, Segment, SegmentOwner};
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

static SPAWNED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static DECAY_WORKER_GENERATION: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);
static DECAY_FINAL_EXIT_GENERATION: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);
static DECAY_STEP_GENERATION: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);
static DECAY_EVENT: OnceLock<(Mutex<()>, Condvar)> = OnceLock::new();

/// Triggers background decay thread initialization.
///
/// Lazily spawns a background worker thread on options initialization if
/// `MNEMOSYNE_PURGE_CADENCE_MS` is non-zero.
pub fn init_decay_engine() {
    let cadence = PURGE_CADENCE_MS.load(Ordering::Acquire);
    // The claim is an unconditional `AcqRel` read-modify-write (no plain-load
    // fast path): every spawn attempt and the purger's shutdown handshake in
    // `decay_thread_loop` then meet as RMWs in `SPAWNED`'s single
    // modification order, and the acquire/release pairing between them is
    // what carries a caller's preceding `PURGE_CADENCE_MS` store into the
    // dying thread's re-check. A plain `load` fast path here could observe a
    // stale `true` from a purger that is concurrently shutting down, skip the
    // spawn *without* creating that edge, and leave the cadence store
    // invisible to the dying thread — the lost-wakeup race the handshake
    // exists to close. `init_decay_engine` is a cold configuration path, so
    // the RMW cost is irrelevant.
    if cadence > 0 && !SPAWNED.swap(true, Ordering::AcqRel) {
        let worker_generation = DECAY_WORKER_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
        thread::Builder::new()
            .name("mnemosyne-decay".to_string())
            .spawn(move || {
                decay_thread_loop(cadence, worker_generation);
            })
            .expect("Failed to spawn mnemosyne-decay thread");
    }
}

fn decay_thread_loop(mut cadence: usize, worker_generation: usize) {
    loop {
        thread::sleep(Duration::from_millis(cadence as u64));

        decay_step();

        cadence = PURGE_CADENCE_MS.load(Ordering::Acquire);
        if cadence != 0 {
            continue;
        }

        // Shutdown handshake. A naive `SPAWNED.store(false) + break` loses a
        // wakeup: a concurrent `configure()` can store a non-zero cadence and
        // call `init_decay_engine` *between* the zero read above and the
        // release of `SPAWNED` — its swap observes `SPAWNED == true`, skips
        // the spawn, and this thread then exits, leaving `SPAWNED == false`
        // with a non-zero cadence and no purger running.
        //
        // The handshake closes every interleaving. Release ownership with an
        // `AcqRel` RMW, then re-check the cadence and try to re-claim:
        // - If a concurrent `init_decay_engine` swap preceded this `swap` in
        //   `SPAWNED`'s modification order, it read `true` and skipped the
        //   spawn; this acquire RMW reads the `true` that swap wrote, so the
        //   caller's earlier cadence store happens-before the re-load below —
        //   the non-zero cadence is observed, the CAS finds the `false` this
        //   thread just wrote, succeeds, and the loop continues. No purger is
        //   lost.
        // - If this `swap` preceded the concurrent one, that swap reads
        //   `false` and spawns a fresh purger; the CAS below then finds
        //   `true` and fails, so this thread exits. Exactly one purger runs.
        // - With no concurrent caller the re-load sees the same zero and the
        //   CAS is never attempted; a later `init_decay_engine` reads the
        //   released `false` and spawns normally.
        let was_spawned = SPAWNED.swap(false, Ordering::AcqRel);
        debug_assert!(
            was_spawned,
            "decay purger exiting without holding the SPAWNED claim"
        );
        cadence = PURGE_CADENCE_MS.load(Ordering::Acquire);
        if cadence != 0
            && SPAWNED
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            // Re-claimed: the cadence became non-zero during shutdown and no
            // replacement thread was spawned; keep purging.
            continue;
        }
        break;
    }

    publish_decay_worker_exit(worker_generation);
}

fn decay_step_event() -> &'static (Mutex<()>, Condvar) {
    DECAY_EVENT.get_or_init(|| (Mutex::new(()), Condvar::new()))
}

/// Returns the number of completed decay sweeps observed by this process.
#[must_use]
pub fn decay_step_generation() -> usize {
    DECAY_STEP_GENERATION.load(Ordering::Acquire)
}

/// Waits for a decay sweep after a previously observed generation.
///
/// The wait is signaled by the background worker after it completes a sweep;
/// it does not poll the segment pools. A `false` result means the supplied
/// timeout elapsed before a later generation was observed.
#[must_use]
pub fn wait_for_decay_step(previous: usize, timeout: Duration) -> bool {
    let (lock, event) = decay_step_event();
    let guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (_guard, _timeout) = event
        .wait_timeout_while(guard, timeout, |_| {
            DECAY_STEP_GENERATION.load(Ordering::Acquire) <= previous
        })
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    DECAY_STEP_GENERATION.load(Ordering::Acquire) > previous
}

/// Waits for the background worker to stop after cadence reaches zero.
///
/// A `true` result means no decay worker is active and the current worker
/// generation has published its final exit. A `false` result means the
/// supplied timeout elapsed before that publication. The function returns
/// immediately when no worker generation is outstanding.
#[must_use]
pub fn wait_for_decay_shutdown(timeout: Duration) -> bool {
    let (lock, event) = decay_step_event();
    let guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (_guard, _timeout) = event
        .wait_timeout_while(guard, timeout, |_| !decay_shutdown_published())
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    decay_shutdown_published()
}

fn decay_shutdown_published() -> bool {
    let worker_generation = DECAY_WORKER_GENERATION.load(Ordering::Acquire);
    let final_exit_generation = DECAY_FINAL_EXIT_GENERATION.load(Ordering::Acquire);
    !SPAWNED.load(Ordering::Acquire) && final_exit_generation >= worker_generation
}

fn publish_decay_step() {
    let (lock, event) = decay_step_event();
    let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    DECAY_STEP_GENERATION.fetch_add(1, Ordering::Release);
    event.notify_all();
}

fn publish_decay_worker_exit(worker_generation: usize) {
    let (lock, event) = decay_step_event();
    let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    DECAY_FINAL_EXIT_GENERATION.fetch_max(worker_generation, Ordering::Release);
    event.notify_all();
}

/// Executes a single decay cycle across all active memory backends.
///
/// Sweeps the global orphan pool for each backend, draining cross-thread
/// frees in idle segments and releasing them back to the OS if empty. Also
/// purges the global segment pool to drop retained free mappings.
pub fn decay_step() {
    // Closed-set maintenance hazard: this list must name every backend whose
    // segment/orphan pools production code can populate. Pool population
    // happens only through a thread allocator, which requires a
    // `LocalAllocatorSelector` impl (`mnemosyne-local/src/lib.rs`), so the
    // swept set is exactly the six production selector backends. A new
    // backend gaining a selector impl MUST be added here, or its orphaned
    // segments and retained mappings are never reclaimed.
    //
    // Per-backend rationale:
    // - `MemoryBackendWrapper`: routing backend of the global allocator
    //   (`Mnemosyne`, `MnemosyneAllocator` default) and the branded
    //   `Heap`/`TieredHeap` host tier — the primary populated pool set.
    // - `CudaUnifiedBackend`/`CudaDeviceBackend`/`CudaHbmBackend`/
    //   `CudaGddrBackend`/`CudaHostPinnedBackend`: device, unified, tier-keyed
    //   device, and pinned pools reachable through
    //   `MnemosyneAllocator<P, B>` and `TieredHeap`'s typed sub-heaps.
    // `DefaultBackend` is intentionally absent: it implements
    // `HasSegmentPool`, but its `LocalAllocatorSelector` impl exists only in
    // `mnemosyne-local`'s test fixtures, so no production thread allocator
    // ever routes through it and its pools stay empty in any process that
    // runs this thread — sweeping it was dead work.
    decay_step_for_backend::<mnemosyne_backend::MemoryBackendWrapper>();
    decay_step_for_backend::<mnemosyne_backend::CudaUnifiedBackend>();
    decay_step_for_backend::<mnemosyne_backend::CudaDeviceBackend>();
    decay_step_for_backend::<mnemosyne_backend::CudaHbmBackend>();
    decay_step_for_backend::<mnemosyne_backend::CudaGddrBackend>();
    decay_step_for_backend::<mnemosyne_backend::CudaHostPinnedBackend>();
    publish_decay_step();
}

fn decay_step_for_backend<B: HasSegmentPool>() {
    decay_orphan_pool::<B>();
    unsafe {
        mnemosyne_arena::purge_segment_pool::<B>();
    }
}

fn decay_orphan_pool<B: HasSegmentPool>() {
    let pool = B::global_orphan_pool();
    let mut retained_head = core::ptr::null_mut::<mnemosyne_core::Segment>();

    // Drain the orphan pool
    while let Some(segment) = pool.pop() {
        // Safety: We popped it from the global pool, so we have exclusive ownership.
        let dynamic_encrypted = unsafe { (*segment).free_list_encrypted };
        let mut total_allocations = 0;

        let mut mask = unsafe { (*segment).page_occupied_mask };
        while mask != 0 {
            let i = mask.trailing_zeros() as usize;
            mask &= mask - 1;
            if i == 0 {
                continue;
            }
            // Addressed through `segment` rather than a `&mut Page`: reclaim
            // reads the segment header for the free-list cookie, and a page
            // borrow held across that access sits on a different provenance.
            let page = unsafe { &raw mut (*segment).pages[i] };
            // Reclaim any cross-thread frees to update the alloc_count, using
            // the segment-aware variant to avoid redundant segment-address masking.
            unsafe {
                Page::reclaim_thread_free_if_present_in_segment(segment, i, dynamic_encrypted);
            }
            total_allocations += unsafe { (*page).alloc_count };
        }

        if total_allocations == 0 {
            // No allocations left! Deallocate segment mapping completely back to OS
            unsafe {
                Segment::set_owner(segment, SegmentOwner::NONE);
                (*segment).next_owned_segment = core::ptr::null_mut();
                (*segment).prev_owned_segment = core::ptr::null_mut();
                mnemosyne_arena::deallocate_segment::<B>(segment);
            }
        } else {
            // Segment still has live allocations, retain it in the local intrusive list
            unsafe {
                (*segment).next_free_segment = retained_head;
            }
            retained_head = segment;
        }
    }

    // Push back retained segments to the orphan pool
    let mut curr = retained_head;
    while !curr.is_null() {
        let next = unsafe { (*curr).next_free_segment };
        unsafe {
            (*curr).next_free_segment = core::ptr::null_mut();
            pool.push_unbounded(curr);
        }
        curr = next;
    }
}
