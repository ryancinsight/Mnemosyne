use core::alloc::{GlobalAlloc, Layout};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread;

// MinGW linker compatibility stubs for snmalloc
#[no_mangle]
pub static mut __imp_VirtualAlloc2FromApp: unsafe extern "system" fn(
    *mut core::ffi::c_void,
    *mut core::ffi::c_void,
    usize,
    u32,
    u32,
    *mut core::ffi::c_void,
    u32,
) -> *mut core::ffi::c_void = fallback_virtual_alloc_2_from_app;

unsafe extern "system" fn fallback_virtual_alloc_2_from_app(
    h_process: *mut core::ffi::c_void,
    base_address: *mut core::ffi::c_void,
    size: usize,
    allocation_type: u32,
    protect: u32,
    extended_parameters: *mut core::ffi::c_void,
    parameter_count: u32,
) -> *mut core::ffi::c_void {
    type FuncType = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        usize,
        u32,
        u32,
        *mut core::ffi::c_void,
        u32,
    ) -> *mut core::ffi::c_void;

    static REAL_FUNC: std::sync::OnceLock<Option<FuncType>> = std::sync::OnceLock::new();

    let func_opt = REAL_FUNC.get_or_init(|| {
        extern "system" {
            fn GetModuleHandleA(lpModuleName: *const u8) -> *mut core::ffi::c_void;
            fn GetProcAddress(
                hModule: *mut core::ffi::c_void,
                lpProcName: *const u8,
            ) -> *mut core::ffi::c_void;
        }

        // Safety: the imported Win32 functions are called with static nul-terminated
        // symbol names and the returned handles are checked for null before use.
        unsafe {
            let kernel32 = GetModuleHandleA(b"kernel32.dll\0".as_ptr());
            if !kernel32.is_null() {
                let func_ptr = GetProcAddress(kernel32, b"VirtualAlloc2FromApp\0".as_ptr());
                if !func_ptr.is_null() {
                    // Safety: `func_ptr` was resolved for `VirtualAlloc2FromApp`,
                    // whose ABI matches `FuncType`.
                    return Some(core::mem::transmute(func_ptr));
                }
                let func_ptr2 = GetProcAddress(kernel32, b"VirtualAlloc2\0".as_ptr());
                if !func_ptr2.is_null() {
                    // Safety: `func_ptr2` was resolved for `VirtualAlloc2`,
                    // whose ABI matches `FuncType`.
                    return Some(core::mem::transmute(func_ptr2));
                }
            }
            None
        }
    });

    if let Some(func) = func_opt {
        // Safety: `func` is a checked dynamic symbol matching `FuncType`.
        unsafe {
            func(
                h_process,
                base_address,
                size,
                allocation_type,
                protect,
                extended_parameters,
                parameter_count,
            )
        }
    } else {
        extern "system" {
            fn VirtualAlloc(
                lpAddress: *mut core::ffi::c_void,
                dwSize: usize,
                flAllocationType: u32,
                flProtect: u32,
            ) -> *mut core::ffi::c_void;
        }
        // Safety: forwards the raw allocation request to the OS fallback with the
        // same parameters supplied to the missing `VirtualAlloc2*` entry point.
        unsafe { VirtualAlloc(base_address, size, allocation_type, protect) }
    }
}

// Safety: all benchmark layouts use nonzero power-of-two alignments and fixed
// positive sizes.
const SMALL_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(32, 8) };
const MEDIUM_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(1024, 8) };
const LARGE_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(8192, 8) };
const BATCH_ALLOCS: usize = 256;
const THREADS: usize = 4;
const THREAD_ALLOCS: usize = 1_000;
const SATURATED_THREAD_ALLOCS: usize = 16_000;
const CROSS_THREAD_ALLOCS: usize = 512;
const CROSS_THREAD_QUEUE_BOUND: usize = 2;
const THREAD_WORK_QUEUE_BOUND: usize = THREADS;
const SEGMENT_EVICTION_ALLOCS: usize = mnemosyne_arena::MAX_RETAINED_SEGMENTS + 8;

#[cold]
fn benchmark_failure(context: &str, detail: &str) -> ! {
    eprintln!("benchmark failure: {context}: {detail}");
    std::process::exit(2);
}

#[inline(always)]
fn require_allocated(ptr: *mut u8, context: &str) -> *mut u8 {
    if ptr.is_null() {
        benchmark_failure(context, "allocator returned a null pointer");
    }
    ptr
}

#[inline(always)]
/// Allocates and deallocates one block for latency benchmarks.
///
/// # Safety
///
/// `layout` must be a valid layout for `allocator`, and the allocator must
/// accept deallocation of pointers it returns for that layout.
unsafe fn alloc_dealloc<A: GlobalAlloc>(allocator: &A, layout: Layout) {
    // Safety: benchmark callers provide a valid `Layout`; null allocation
    // results are rejected before the pointer is handed back to `dealloc`.
    let ptr = require_allocated(allocator.alloc(black_box(layout)), "alloc_dealloc");
    black_box(ptr);
    // Safety: `ptr` was returned by the same allocator for `layout` above.
    allocator.dealloc(ptr, layout);
}

#[inline(never)]
/// Allocates and deallocates a fixed batch for burst-retention benchmarks.
///
/// # Safety
///
/// `layout` must be a valid layout for `allocator`, and each allocation is
/// deallocated exactly once by this function.
unsafe fn burst_alloc_dealloc<A: GlobalAlloc>(allocator: &A, layout: Layout) {
    let mut ptrs = [core::ptr::null_mut(); BATCH_ALLOCS];
    for ptr in &mut ptrs {
        // Safety: benchmark callers provide a valid `Layout`; null allocation
        // results are rejected before storing the pointer for later deallocation.
        *ptr = require_allocated(allocator.alloc(black_box(layout)), "burst_alloc_dealloc");
    }
    black_box(&ptrs);
    for ptr in ptrs {
        // Safety: every pointer in `ptrs` was allocated by `allocator` with
        // `layout` in the loop above and has not yet been deallocated.
        allocator.dealloc(ptr, layout);
    }
}

struct HandoffBatch {
    ptrs: [usize; CROSS_THREAD_ALLOCS],
    layout: Layout,
}

struct HandoffWorker<A: GlobalAlloc + Send + Sync + 'static> {
    allocator: &'static A,
    sender: SyncSender<Option<HandoffBatch>>,
    done: Receiver<()>,
    handle: Option<thread::JoinHandle<()>>,
}

impl<A: GlobalAlloc + Send + Sync + 'static> HandoffWorker<A> {
    fn new(allocator: &'static A) -> Self {
        let (sender, receiver) = sync_channel::<Option<HandoffBatch>>(CROSS_THREAD_QUEUE_BOUND);
        let (done_sender, done) = sync_channel::<()>(CROSS_THREAD_QUEUE_BOUND);
        let handle = thread::spawn(move || {
            while let Ok(Some(batch)) = receiver.recv() {
                for ptr in batch.ptrs {
                    // Safety: each pointer in the batch was allocated by the
                    // same allocator with `batch.layout` before handoff.
                    unsafe {
                        allocator.dealloc(ptr as *mut u8, batch.layout);
                    }
                }
                if done_sender.send(()).is_err() {
                    return;
                }
            }
        });

        Self {
            allocator,
            sender,
            done,
            handle: Some(handle),
        }
    }

    fn alloc_then_handoff(&self, layout: Layout) {
        let mut ptrs = [0usize; CROSS_THREAD_ALLOCS];
        for ptr in &mut ptrs {
            // Safety: `layout` is one of the static benchmark layouts.
            let allocated = unsafe { self.allocator.alloc(black_box(layout)) };
            require_allocated(allocated, "cross-thread handoff allocation");
            *ptr = allocated as usize;
        }
        black_box(&ptrs);
        if self
            .sender
            .send(Some(HandoffBatch { ptrs, layout }))
            .is_err()
        {
            benchmark_failure("cross-thread handoff", "worker command channel closed");
        }
        if self.done.recv().is_err() {
            benchmark_failure("cross-thread handoff", "worker completion channel closed");
        }
    }
}

impl<A: GlobalAlloc + Send + Sync + 'static> Drop for HandoffWorker<A> {
    fn drop(&mut self) {
        let _ = self.sender.send(None);
        if let Some(handle) = self.handle.take() {
            if handle.join().is_err() {
                eprintln!(
                    "benchmark failure: cross-thread handoff worker panicked during shutdown"
                );
            }
        }
    }
}

struct ThreadCycleWorkers<A: GlobalAlloc + Send + Sync + 'static> {
    senders: Vec<SyncSender<Option<usize>>>,
    done: Receiver<()>,
    handles: Vec<thread::JoinHandle<()>>,
    _allocator: &'static A,
}

impl<A: GlobalAlloc + Send + Sync + 'static> ThreadCycleWorkers<A> {
    fn new(allocator: &'static A) -> Self {
        let (done_sender, done) = sync_channel::<()>(THREAD_WORK_QUEUE_BOUND);
        let mut senders = Vec::with_capacity(THREADS);
        let mut handles = Vec::with_capacity(THREADS);
        for _ in 0..THREADS {
            let (sender, receiver) = sync_channel::<Option<usize>>(THREAD_WORK_QUEUE_BOUND);
            let worker_done = done_sender.clone();
            let handle_allocator = allocator;
            let handle = thread::spawn(move || {
                while let Ok(Some(iterations)) = receiver.recv() {
                    for _ in 0..iterations {
                        // Safety: `SMALL_LAYOUT` is a valid static layout and
                        // `alloc_dealloc` validates non-null allocations.
                        unsafe {
                            alloc_dealloc(handle_allocator, SMALL_LAYOUT);
                        }
                    }
                    if worker_done.send(()).is_err() {
                        return;
                    }
                }
            });
            senders.push(sender);
            handles.push(handle);
        }

        Self {
            senders,
            done,
            handles,
            _allocator: allocator,
        }
    }

    fn run(&self) {
        self.run_with_iterations(THREAD_ALLOCS);
    }

    fn run_with_iterations(&self, iterations: usize) {
        for sender in &self.senders {
            if sender.send(Some(iterations)).is_err() {
                benchmark_failure("threaded allocation cycle", "worker command channel closed");
            }
        }
        for _ in 0..THREADS {
            if self.done.recv().is_err() {
                benchmark_failure(
                    "threaded allocation cycle",
                    "worker completion channel closed",
                );
            }
        }
    }
}

impl<A: GlobalAlloc + Send + Sync + 'static> Drop for ThreadCycleWorkers<A> {
    fn drop(&mut self) {
        for sender in &self.senders {
            let _ = sender.send(None);
        }
        for handle in self.handles.drain(..) {
            if handle.join().is_err() {
                eprintln!("benchmark failure: allocation-cycle worker panicked during shutdown");
            }
        }
    }
}

#[inline(never)]
/// Exercises the segment cache retention and purge boundary.
///
/// # Safety
///
/// Every segment returned by the arena allocator is retained in the local
/// array and deallocated exactly once before the function returns.
unsafe fn segment_cache_eviction_cycle() {
    let mut segments = [core::ptr::null_mut::<mnemosyne_core::Segment>(); SEGMENT_EVICTION_ALLOCS];
    for segment in &mut segments {
        // Safety: benchmark owns every returned segment pointer until it is
        // deallocated later in this function.
        *segment =
            match mnemosyne_arena::allocate_segment::<mnemosyne_backend::MemoryBackendWrapper>() {
                Some(segment) => segment,
                None => benchmark_failure("segment cache eviction", "segment allocation failed"),
            };
    }
    black_box(&segments);
    for segment in segments {
        // Safety: each `segment` was allocated above and is deallocated exactly once.
        mnemosyne_arena::deallocate_segment::<mnemosyne_backend::MemoryBackendWrapper>(segment);
    }
    let stats = mnemosyne_arena::arena_memory_stats::<mnemosyne_backend::MemoryBackendWrapper>();
    if stats.retained_free_segments > stats.max_retained_free_segments {
        benchmark_failure(
            "segment cache eviction",
            "retained free segments exceeded configured maximum",
        );
    }
}

fn bench_allocator_cycles(c: &mut Criterion) {
    let mut group = c.benchmark_group("Allocator cycle latency");
    for (name, layout) in [
        ("small/32", SMALL_LAYOUT),
        ("medium/1024", MEDIUM_LAYOUT),
        ("large/8192", LARGE_LAYOUT),
    ] {
        group.throughput(Throughput::Bytes(layout.size() as u64));
        group.bench_with_input(BenchmarkId::new("Mnemosyne", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { alloc_dealloc(&mnemosyne::Mnemosyne, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("MiMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { alloc_dealloc(&mimalloc::MiMalloc, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("SnMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { alloc_dealloc(&snmalloc_rs::SnMalloc, *layout) })
        });
    }
    group.finish();
}

fn bench_allocator_bursts(c: &mut Criterion) {
    let mut group = c.benchmark_group("Allocator burst retention");
    for (name, layout) in [
        ("small/32", SMALL_LAYOUT),
        ("medium/1024", MEDIUM_LAYOUT),
        ("large/8192", LARGE_LAYOUT),
    ] {
        group.throughput(Throughput::Bytes((layout.size() * BATCH_ALLOCS) as u64));
        group.bench_with_input(BenchmarkId::new("Mnemosyne", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { burst_alloc_dealloc(&mnemosyne::Mnemosyne, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("MiMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { burst_alloc_dealloc(&mimalloc::MiMalloc, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("SnMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { burst_alloc_dealloc(&snmalloc_rs::SnMalloc, *layout) })
        });
    }
    group.finish();
}

fn bench_cross_thread_free(c: &mut Criterion) {
    static MNEMOSYNE: mnemosyne::Mnemosyne = mnemosyne::Mnemosyne;
    static MIMALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;
    static SNMALLOC: snmalloc_rs::SnMalloc = snmalloc_rs::SnMalloc;

    let mut group = c.benchmark_group("Cross-thread free handoff");
    for (name, layout) in [("small/32", SMALL_LAYOUT), ("medium/1024", MEDIUM_LAYOUT)] {
        group.throughput(Throughput::Elements(CROSS_THREAD_ALLOCS as u64));
        let mnemosyne_worker = HandoffWorker::new(&MNEMOSYNE);
        group.bench_with_input(BenchmarkId::new("Mnemosyne", name), &layout, |b, layout| {
            b.iter(|| mnemosyne_worker.alloc_then_handoff(*layout))
        });
        drop(mnemosyne_worker);

        let mimalloc_worker = HandoffWorker::new(&MIMALLOC);
        group.bench_with_input(BenchmarkId::new("MiMalloc", name), &layout, |b, layout| {
            b.iter(|| mimalloc_worker.alloc_then_handoff(*layout))
        });
        drop(mimalloc_worker);

        let snmalloc_worker = HandoffWorker::new(&SNMALLOC);
        group.bench_with_input(BenchmarkId::new("SnMalloc", name), &layout, |b, layout| {
            b.iter(|| snmalloc_worker.alloc_then_handoff(*layout))
        });
        drop(snmalloc_worker);
    }

    let stats = mnemosyne::memory_stats();
    if stats.retained_free_segments > stats.max_retained_free_segments {
        benchmark_failure(
            "cross-thread free handoff",
            "retained free segments exceeded configured maximum",
        );
    }
    black_box(stats);
    group.finish();
}

fn bench_multithreaded_alloc(c: &mut Criterion) {
    static MNEMOSYNE: mnemosyne::Mnemosyne = mnemosyne::Mnemosyne;
    static MIMALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;
    static SNMALLOC: snmalloc_rs::SnMalloc = snmalloc_rs::SnMalloc;

    let mut group = c.benchmark_group("Threaded small allocation cycles");
    group.throughput(Throughput::Elements((THREADS * THREAD_ALLOCS) as u64));

    let mnemosyne_workers = ThreadCycleWorkers::new(&MNEMOSYNE);
    group.bench_function("Mnemosyne", |b| b.iter(|| mnemosyne_workers.run()));
    drop(mnemosyne_workers);

    let mimalloc_workers = ThreadCycleWorkers::new(&MIMALLOC);
    group.bench_function("MiMalloc", |b| b.iter(|| mimalloc_workers.run()));
    drop(mimalloc_workers);

    let snmalloc_workers = ThreadCycleWorkers::new(&SNMALLOC);
    group.bench_function("SnMalloc", |b| b.iter(|| snmalloc_workers.run()));
    drop(snmalloc_workers);

    group.finish();
}

fn bench_saturated_multithreaded_alloc(c: &mut Criterion) {
    static MNEMOSYNE: mnemosyne::Mnemosyne = mnemosyne::Mnemosyne;
    static MIMALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;
    static SNMALLOC: snmalloc_rs::SnMalloc = snmalloc_rs::SnMalloc;

    let mut group = c.benchmark_group("Threaded saturated small allocation cycles");
    group.throughput(Throughput::Elements(
        (THREADS * SATURATED_THREAD_ALLOCS) as u64,
    ));

    let mnemosyne_workers = ThreadCycleWorkers::new(&MNEMOSYNE);
    group.bench_function("Mnemosyne", |b| {
        b.iter(|| mnemosyne_workers.run_with_iterations(SATURATED_THREAD_ALLOCS))
    });
    drop(mnemosyne_workers);

    let mimalloc_workers = ThreadCycleWorkers::new(&MIMALLOC);
    group.bench_function("MiMalloc", |b| {
        b.iter(|| mimalloc_workers.run_with_iterations(SATURATED_THREAD_ALLOCS))
    });
    drop(mimalloc_workers);

    let snmalloc_workers = ThreadCycleWorkers::new(&SNMALLOC);
    group.bench_function("SnMalloc", |b| {
        b.iter(|| snmalloc_workers.run_with_iterations(SATURATED_THREAD_ALLOCS))
    });
    drop(snmalloc_workers);

    group.finish();
}

fn bench_segment_cache_eviction(c: &mut Criterion) {
    // Safety: benchmark setup clears only Mnemosyne's reusable segment pool.
    unsafe {
        mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
    }

    let mut group = c.benchmark_group("Segment cache eviction");
    group.throughput(Throughput::Elements(SEGMENT_EVICTION_ALLOCS as u64));
    group.bench_function("Mnemosyne", |b| {
        // Safety: `segment_cache_eviction_cycle` owns every allocated segment.
        b.iter(|| unsafe { segment_cache_eviction_cycle() })
    });
    group.finish();

    // Safety: benchmark teardown clears only Mnemosyne's reusable segment pool.
    unsafe {
        mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
    }
}

criterion_group!(
    benches,
    bench_allocator_cycles,
    bench_allocator_bursts,
    bench_cross_thread_free,
    bench_multithreaded_alloc,
    bench_saturated_multithreaded_alloc,
    bench_segment_cache_eviction
);
criterion_main!(benches);
