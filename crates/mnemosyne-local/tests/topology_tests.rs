use core::alloc::Layout;
use mnemosyne_core::StandardPolicy;

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct PerCpuCacheGuard;

impl PerCpuCacheGuard {
    fn enable() -> Self {
        mnemosyne_local::per_cpu::PER_CPU_CACHE_ENABLED
            .store(true, core::sync::atomic::Ordering::Relaxed);
        Self
    }
}

impl Drop for PerCpuCacheGuard {
    fn drop(&mut self) {
        mnemosyne_local::per_cpu::PER_CPU_CACHE_ENABLED
            .store(false, core::sync::atomic::Ordering::Relaxed);
    }
}

#[test]
fn test_per_cpu_cache() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local topology test lock was poisoned");
    use mnemosyne_backend::MemoryBackendWrapper;
    use mnemosyne_local::{per_cpu, thread_alloc, thread_free};

    let _per_cpu_guard = PerCpuCacheGuard::enable();
    let layout = Layout::from_size_align(16, 8)
        .expect("16-byte allocation with 8-byte alignment is a valid Layout");

    // Allocate a block via thread_alloc
    let ptr = unsafe {
        thread_alloc::<StandardPolicy, MemoryBackendWrapper>(layout.size(), layout.align())
    };
    assert!(!ptr.is_null());

    // Try to free to the CPU cache
    let freed = per_cpu::try_free_cpu::<StandardPolicy>(ptr, 0);
    assert!(freed);

    // Pop it back from the CPU cache
    let popped = per_cpu::try_alloc_cpu::<StandardPolicy>(0);
    assert_eq!(popped, ptr);

    unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(popped) };
}

#[test]
fn test_numa_node_segment_retention() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local topology test lock was poisoned");
    use mnemosyne_arena::current_numa_node;
    let node = current_numa_node();
    println!("Current NUMA node: {}", node);

    // Verify segment allocation sets numa_node
    unsafe {
        let segment =
            mnemosyne_arena::allocate_segment::<mnemosyne_backend::MemoryBackendWrapper>()
                .expect("OS-backed segment allocation must succeed for topology test");
        assert_eq!((*segment).numa_node, node);
        mnemosyne_arena::deallocate_segment::<mnemosyne_backend::MemoryBackendWrapper>(segment);
    }
}

#[test]
fn test_per_cpu_cache_contention_bounds() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local topology test lock was poisoned");
    use core::sync::atomic::{AtomicBool, Ordering};
    use mnemosyne_local::per_cpu;
    use std::thread;

    let _per_cpu_guard = PerCpuCacheGuard::enable();
    let cpu_id = per_cpu::get_current_cpu_id();
    let class = 0;

    // Set up a block in the per-CPU cache slot
    let dummy_block = 0x12345678usize;
    per_cpu::PER_CPU_CACHE.slots[cpu_id].blocks[class][0].store(dummy_block, Ordering::Relaxed);

    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();

    // Spawn a thread to constantly change the slot value to cause CAS failures
    let handle = thread::spawn(move || {
        let mut val = dummy_block;
        while !stop_clone.load(Ordering::Relaxed) {
            val = if val == dummy_block {
                0x87654321usize
            } else {
                dummy_block
            };
            per_cpu::PER_CPU_CACHE.slots[cpu_id].blocks[class][0].store(val, Ordering::Relaxed);
        }
    });

    // Run try_alloc_cpu many times under severe contention.
    // It must return quickly (either returning a block or null) and NEVER hang/live-lock.
    for _ in 0..1000 {
        let _res = per_cpu::try_alloc_cpu::<StandardPolicy>(class);
    }

    stop.store(true, Ordering::Relaxed);
    let _ = handle.join();

    // Clean up slot
    per_cpu::PER_CPU_CACHE.slots[cpu_id].blocks[class][0].store(0, Ordering::Relaxed);
}
