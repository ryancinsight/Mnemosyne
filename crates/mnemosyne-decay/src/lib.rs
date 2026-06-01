use core::sync::atomic::Ordering;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::constants::PAGES_PER_SEGMENT;
use mnemosyne_core::options::PURGE_CADENCE_MS;
use mnemosyne_core::types::SegmentOwner;
use std::thread;
use std::time::Duration;

static SPAWNED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Triggers background decay thread initialization.
///
/// Lazily spawns a background worker thread on options initialization if
/// `MNEMOSYNE_PURGE_CADENCE_MS` is non-zero.
pub fn init_decay_engine() {
    if !SPAWNED.load(Ordering::Acquire) {
        let cadence = PURGE_CADENCE_MS.load(Ordering::Acquire);
        if cadence > 0 && !SPAWNED.swap(true, Ordering::AcqRel) {
            thread::Builder::new()
                .name("mnemosyne-decay".to_string())
                .spawn(move || {
                    decay_thread_loop(cadence);
                })
                .expect("Failed to spawn mnemosyne-decay thread");
        }
    }
}

fn decay_thread_loop(mut cadence: usize) {
    loop {
        thread::sleep(Duration::from_millis(cadence as u64));

        decay_step();

        cadence = PURGE_CADENCE_MS.load(Ordering::Acquire);
        if cadence == 0 {
            SPAWNED.store(false, Ordering::Release);
            break;
        }
    }
}

/// Executes a single decay cycle across all active memory backends.
///
/// Sweeps the global orphan pool for each backend, draining cross-thread
/// frees in idle segments and releasing them back to the OS if empty. Also
/// purges the global segment pool to drop retained free mappings.
pub fn decay_step() {
    decay_step_for_backend::<mnemosyne_backend::DefaultBackend>();
    decay_step_for_backend::<mnemosyne_backend::MemoryBackendWrapper>();
    decay_step_for_backend::<mnemosyne_backend::CudaUnifiedBackend>();
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

        for i in 1..PAGES_PER_SEGMENT {
            let page = unsafe { &mut (*segment).pages[i] };
            if page.block_size > 0 {
                // Reclaim any cross-thread frees to update the alloc_count
                unsafe {
                    page.reclaim_thread_free_dynamic(dynamic_encrypted);
                }
                total_allocations += page.alloc_count;
            }
        }

        if total_allocations == 0 {
            // No allocations left! Deallocate segment mapping completely back to OS
            unsafe {
                (*segment).owner = SegmentOwner::NONE;
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
