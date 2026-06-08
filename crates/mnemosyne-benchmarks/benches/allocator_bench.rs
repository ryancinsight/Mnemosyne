use core::alloc::{GlobalAlloc, Layout};
use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use std::alloc::System;
use std::cell::UnsafeCell;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Jemalloc comparator abstraction.
///
/// Exposes a single `Jemalloc` `GlobalAlloc` and a `usable_size` helper so the
/// benchmark bodies are identical across platforms. On non-Windows targets this
/// is `tikv-jemallocator`; on Windows (under the `system-jemalloc` feature) it
/// is a thin `GlobalAlloc` over a system-installed `libjemalloc_s.a`, using
/// jemalloc's sized `je_*x` API for parity with `tikv-jemallocator`. The module
/// only exists when `build.rs` determined jemalloc is available
/// (`jemalloc_available` cfg); every use site is gated the same way.
#[cfg(jemalloc_available)]
mod bench_jemalloc {
    #[cfg(not(windows))]
    pub use tikv_jemallocator::{usable_size, Jemalloc};

    #[cfg(windows)]
    pub use sys::{usable_size, SystemJemalloc as Jemalloc};

    #[cfg(windows)]
    mod sys {
        use core::alloc::{GlobalAlloc, Layout};

        // Link the system jemalloc static library directly from this object.
        // `build.rs` emits the `-L` search path (e.g. MSYS2 `ucrt64/lib`); the
        // `#[link]` attribute embeds the `-l` requirement in the bench crate
        // itself, which is more reliable than build-script `rustc-link-lib`
        // propagation across the separate bench crate.
        #[link(name = "jemalloc_s", kind = "static")]
        extern "C" {
            fn je_mallocx(size: usize, flags: i32) -> *mut u8;
            fn je_rallocx(ptr: *mut u8, size: usize, flags: i32) -> *mut u8;
            fn je_sdallocx(ptr: *mut u8, size: usize, flags: i32);
            fn je_malloc_usable_size(ptr: *const u8) -> usize;
        }

        /// jemalloc `MALLOCX_ZERO` flag (request zeroed memory).
        const MALLOCX_ZERO: i32 = 0x40;

        /// Encodes the jemalloc `mallocx`/`sdallocx`/`rallocx` flags for a
        /// layout. Mirrors `tikv-jemallocator`'s `layout_to_flags`: no
        /// alignment flag when the alignment is within jemalloc's natural
        /// size-class guarantee (`<= 16` on 64-bit and `<= size`); otherwise
        /// `MALLOCX_ALIGN(align)`, which is `log2(align)` for power-of-two
        /// alignments.
        #[inline]
        fn flags(layout: Layout) -> i32 {
            let align = layout.align();
            if align <= 16 && align <= layout.size() {
                0
            } else {
                align.trailing_zeros() as i32
            }
        }

        /// `GlobalAlloc` over the system jemalloc static library.
        pub struct SystemJemalloc;

        // Safety: jemalloc's allocator is thread-safe and satisfies the
        // `GlobalAlloc` contract; the sized `je_*x` calls forward layout size
        // and alignment exactly.
        unsafe impl GlobalAlloc for SystemJemalloc {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                je_mallocx(layout.size(), flags(layout))
            }

            unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
                je_mallocx(layout.size(), flags(layout) | MALLOCX_ZERO)
            }

            unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
                je_sdallocx(ptr, layout.size(), flags(layout));
            }

            unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
                // Safety: new_size is nonzero and align is the original
                // power-of-two alignment, a valid layout.
                let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
                je_rallocx(ptr, new_size, flags(new_layout))
            }
        }

        /// Mirrors `tikv_jemallocator::usable_size`.
        ///
        /// # Safety
        ///
        /// `ptr` must have been returned by this allocator and still be live.
        pub unsafe fn usable_size<T>(ptr: *const T) -> usize {
            je_malloc_usable_size(ptr as *const u8)
        }
    }
}

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
            let kernel32 = GetModuleHandleA(c"kernel32.dll".as_ptr() as *const u8);
            if !kernel32.is_null() {
                let func_ptr =
                    GetProcAddress(kernel32, c"VirtualAlloc2FromApp".as_ptr() as *const u8);
                if !func_ptr.is_null() {
                    // Safety: `func_ptr` was resolved for `VirtualAlloc2FromApp`,
                    // whose ABI matches `FuncType`.
                    return Some(core::mem::transmute::<*mut core::ffi::c_void, FuncType>(
                        func_ptr,
                    ));
                }
                let func_ptr2 = GetProcAddress(kernel32, c"VirtualAlloc2".as_ptr() as *const u8);
                if !func_ptr2.is_null() {
                    // Safety: `func_ptr2` was resolved for `VirtualAlloc2`,
                    // whose ABI matches `FuncType`.
                    return Some(core::mem::transmute::<*mut core::ffi::c_void, FuncType>(
                        func_ptr2,
                    ));
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
const SMALL_WITHIN_CLASS_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(24, 8) };
const MEDIUM_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(1024, 8) };
const LARGE_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(8192, 8) };
const LARGE_WITHIN_CLASS_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(6144, 8) };
const HUGE_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(2 * 1024 * 1024, 4096) };
const HUGE_REALLOC_SRC_LAYOUT: Layout =
    unsafe { Layout::from_size_align_unchecked(4 * 1024 * 1024, 4096) };
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

struct AllocatedBlock<'a, A: GlobalAlloc> {
    allocator: &'a A,
    ptr: *mut u8,
    layout: Layout,
}

impl<'a, A: GlobalAlloc> AllocatedBlock<'a, A> {
    #[inline(always)]
    unsafe fn new(allocator: &'a A, layout: Layout, context: &str) -> Self {
        let ptr = require_allocated(allocator.alloc(black_box(layout)), context);
        Self {
            allocator,
            ptr,
            layout,
        }
    }
}

impl<A: GlobalAlloc> Drop for AllocatedBlock<'_, A> {
    fn drop(&mut self) {
        // Safety: `ptr` was allocated by `allocator` for `layout` in `new`.
        unsafe { self.allocator.dealloc(self.ptr, self.layout) };
    }
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
    let ptr = black_box(ptr);
    // Safety: `ptr` was returned by the same allocator for `layout` above.
    allocator.dealloc(ptr, layout);
}

#[inline(always)]
/// Deallocates a pointer allocated by the same allocator during benchmark setup.
///
/// # Safety
///
/// `ptr` must be non-null, allocated by `allocator` for `layout`, and not
/// deallocated elsewhere.
unsafe fn dealloc_only<A: GlobalAlloc>(allocator: &A, ptr: *mut u8, layout: Layout) {
    allocator.dealloc(black_box(ptr), layout);
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

#[inline(always)]
/// Allocates one block, queries the allocator's usable-size API, and
/// deallocates the block.
///
/// # Safety
///
/// `layout` must be valid for `allocator`, `usable_size` must accept only
/// pointers returned by that allocator, and deallocation must use the same
/// layout used for allocation.
unsafe fn alloc_usable_dealloc<A, F>(allocator: &A, layout: Layout, usable_size: F)
where
    A: GlobalAlloc,
    F: Fn(*mut u8) -> usize,
{
    // Safety: benchmark callers provide a valid `Layout`; null allocation
    // results are rejected before either usable-size probing or deallocation.
    let ptr = require_allocated(allocator.alloc(black_box(layout)), "alloc_usable_dealloc");
    let ptr = black_box(ptr);
    let size = usable_size(ptr);
    if size < layout.size() {
        benchmark_failure(
            "alloc_usable_dealloc",
            "usable size was smaller than allocation layout",
        );
    }
    black_box(size);
    // Safety: `ptr` was returned by the same allocator for `layout` above.
    allocator.dealloc(black_box(ptr), layout);
}

#[inline(always)]
/// Allocates one block, reallocates it to `new_size`, and deallocates the
/// resulting block.
///
/// # Safety
///
/// `old_layout` must be valid for `allocator`, `new_size` must be valid
/// with `old_layout.align()`, and the allocator must accept deallocation
/// of the pointer returned by `realloc` with the derived new layout.
unsafe fn alloc_realloc_dealloc<A: GlobalAlloc>(
    allocator: &A,
    old_layout: Layout,
    new_size: usize,
) {
    // Safety: benchmark callers provide a valid `Layout`; null allocation
    // results are rejected before the pointer is passed to realloc.
    let ptr = require_allocated(
        allocator.alloc(black_box(old_layout)),
        "alloc_realloc_dealloc",
    );
    // Safety: benchmark constants use valid size/alignment pairs.
    let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, old_layout.align()) };
    // Safety: `ptr` was returned by `allocator` for `old_layout`, and
    // `new_size` is valid for `old_layout.align()`.
    let new_ptr = require_allocated(
        allocator.realloc(ptr, old_layout, black_box(new_size)),
        "alloc_realloc_dealloc",
    );
    black_box(new_ptr);
    // Safety: `new_ptr` was returned by the same allocator's realloc call.
    allocator.dealloc(new_ptr, new_layout);
}

struct HandoffBatch {
    layout: Layout,
    count: usize,
}

struct HandoffBuffer {
    slots: UnsafeCell<[usize; CROSS_THREAD_ALLOCS]>,
}

unsafe impl Sync for HandoffBuffer {}

impl HandoffBuffer {
    #[inline]
    fn new() -> Self {
        Self {
            slots: UnsafeCell::new([0; CROSS_THREAD_ALLOCS]),
        }
    }

    #[inline(always)]
    unsafe fn write(&self, index: usize, ptr: usize) {
        debug_assert!(index < CROSS_THREAD_ALLOCS);
        unsafe {
            (*self.slots.get())[index] = ptr;
        }
    }

    #[inline(always)]
    unsafe fn read(&self, index: usize) -> usize {
        debug_assert!(index < CROSS_THREAD_ALLOCS);
        unsafe { (*self.slots.get())[index] }
    }
}

struct HandoffWorker<A: GlobalAlloc + Send + Sync + 'static> {
    allocator: &'static A,
    buffer: Arc<HandoffBuffer>,
    sender: SyncSender<Option<HandoffBatch>>,
    done: Receiver<()>,
    handle: Option<thread::JoinHandle<()>>,
}

impl<A: GlobalAlloc + Send + Sync + 'static> HandoffWorker<A> {
    fn new(allocator: &'static A) -> Self {
        let buffer = Arc::new(HandoffBuffer::new());
        let worker_buffer = Arc::clone(&buffer);
        let (sender, receiver) = sync_channel::<Option<HandoffBatch>>(CROSS_THREAD_QUEUE_BOUND);
        let (done_sender, done) = sync_channel::<()>(CROSS_THREAD_QUEUE_BOUND);
        let handle = thread::spawn(move || {
            while let Ok(Some(batch)) = receiver.recv() {
                for index in 0..batch.count {
                    // Safety: the producer writes exactly `batch.count`
                    // initialized slots before sending this command, and waits
                    // for `done` before reusing the buffer.
                    let ptr = unsafe { worker_buffer.read(index) };
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
            buffer,
            sender,
            done,
            handle: Some(handle),
        }
    }

    fn alloc_then_handoff(&self, layout: Layout, count: usize) {
        debug_assert!(count <= CROSS_THREAD_ALLOCS);
        for index in 0..count {
            // Safety: `layout` is one of the static benchmark layouts.
            let allocated = unsafe { self.allocator.alloc(black_box(layout)) };
            require_allocated(allocated, "cross-thread handoff allocation");
            // Safety: `index < count <= CROSS_THREAD_ALLOCS`, and the worker
            // cannot read this buffer until the command is sent below.
            unsafe { self.buffer.write(index, allocated as usize) };
        }
        black_box(&self.buffer);
        if self
            .sender
            .send(Some(HandoffBatch { layout, count }))
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

struct ThreadCycleWorker {
    sender: SyncSender<Option<usize>>,
    handle: Option<thread::JoinHandle<()>>,
}

struct ThreadCycleWorkers<A: GlobalAlloc + Send + Sync + 'static> {
    workers: [ThreadCycleWorker; THREADS],
    done: Receiver<()>,
    _allocator: &'static A,
}

impl<A: GlobalAlloc + Send + Sync + 'static> ThreadCycleWorkers<A> {
    fn new(allocator: &'static A, layout: Layout) -> Self {
        let (done_sender, done) = sync_channel::<()>(THREAD_WORK_QUEUE_BOUND);
        let workers = std::array::from_fn(|_| {
            let (sender, receiver) = sync_channel::<Option<usize>>(THREAD_WORK_QUEUE_BOUND);
            let worker_done = done_sender.clone();
            let handle_allocator = allocator;
            let handle = thread::spawn(move || {
                while let Ok(Some(iterations)) = receiver.recv() {
                    for _ in 0..iterations {
                        // Safety: `layout` is a valid static layout and
                        // `alloc_dealloc` validates non-null allocations.
                        unsafe {
                            alloc_dealloc(handle_allocator, layout);
                        }
                    }
                    if worker_done.send(()).is_err() {
                        return;
                    }
                }
            });
            ThreadCycleWorker {
                sender,
                handle: Some(handle),
            }
        });

        Self {
            workers,
            done,
            _allocator: allocator,
        }
    }

    fn run(&self) {
        self.run_with_iterations(THREAD_ALLOCS);
    }

    fn run_with_iterations(&self, iterations: usize) {
        for worker in &self.workers {
            if worker.sender.send(Some(iterations)).is_err() {
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
        for worker in &self.workers {
            let _ = worker.sender.send(None);
        }
        for worker in &mut self.workers {
            if let Some(handle) = worker.handle.take() {
                if handle.join().is_err() {
                    eprintln!(
                        "benchmark failure: allocation-cycle worker panicked during shutdown"
                    );
                }
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
        ("huge/2m", HUGE_LAYOUT),
    ] {
        group.throughput(Throughput::Bytes(layout.size() as u64));
        group.bench_with_input(BenchmarkId::new("Mnemosyne", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { alloc_dealloc(&mnemosyne::Mnemosyne, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("System", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { alloc_dealloc(&System, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("MiMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { alloc_dealloc(&mimalloc::MiMalloc, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("RpMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { alloc_dealloc(&rpmalloc::RpMalloc, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("SnMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { alloc_dealloc(&snmalloc_rs::SnMalloc, *layout) })
        });
        #[cfg(jemalloc_available)]
        {
            group.bench_with_input(BenchmarkId::new("Jemalloc", name), &layout, |b, layout| {
                // Safety: `layout` comes from the static valid benchmark layout table.
                b.iter(|| unsafe { alloc_dealloc(&bench_jemalloc::Jemalloc, *layout) })
            });
        }
    }
    group.finish();
}

fn bench_allocator_alloc(c: &mut Criterion) {
    let mut group = c.benchmark_group("Allocator allocation latency");
    for (name, layout) in [
        ("small/32", SMALL_LAYOUT),
        ("medium/1024", MEDIUM_LAYOUT),
        ("large/8192", LARGE_LAYOUT),
        ("huge/2m", HUGE_LAYOUT),
    ] {
        group.throughput(Throughput::Bytes(layout.size() as u64));
        group.bench_with_input(BenchmarkId::new("Mnemosyne", name), &layout, |b, layout| {
            b.iter_batched(
                || (),
                |_| unsafe { AllocatedBlock::new(&mnemosyne::Mnemosyne, *layout, "alloc_only") },
                BatchSize::SmallInput,
            )
        });
        group.bench_with_input(BenchmarkId::new("System", name), &layout, |b, layout| {
            b.iter_batched(
                || (),
                |_| unsafe { AllocatedBlock::new(&System, *layout, "alloc_only") },
                BatchSize::SmallInput,
            )
        });
        group.bench_with_input(BenchmarkId::new("MiMalloc", name), &layout, |b, layout| {
            b.iter_batched(
                || (),
                |_| unsafe { AllocatedBlock::new(&mimalloc::MiMalloc, *layout, "alloc_only") },
                BatchSize::SmallInput,
            )
        });
        group.bench_with_input(BenchmarkId::new("RpMalloc", name), &layout, |b, layout| {
            b.iter_batched(
                || (),
                |_| unsafe { AllocatedBlock::new(&rpmalloc::RpMalloc, *layout, "alloc_only") },
                BatchSize::SmallInput,
            )
        });
        group.bench_with_input(BenchmarkId::new("SnMalloc", name), &layout, |b, layout| {
            b.iter_batched(
                || (),
                |_| unsafe { AllocatedBlock::new(&snmalloc_rs::SnMalloc, *layout, "alloc_only") },
                BatchSize::SmallInput,
            )
        });
        #[cfg(jemalloc_available)]
        {
            group.bench_with_input(BenchmarkId::new("Jemalloc", name), &layout, |b, layout| {
                b.iter_batched(
                    || (),
                    |_| unsafe {
                        AllocatedBlock::new(&bench_jemalloc::Jemalloc, *layout, "alloc_only")
                    },
                    BatchSize::SmallInput,
                )
            });
        }
    }
    group.finish();
}

fn bench_allocator_dealloc(c: &mut Criterion) {
    let mut group = c.benchmark_group("Allocator deallocation latency");
    for (name, layout) in [
        ("small/32", SMALL_LAYOUT),
        ("medium/1024", MEDIUM_LAYOUT),
        ("large/8192", LARGE_LAYOUT),
        ("huge/2m", HUGE_LAYOUT),
    ] {
        group.throughput(Throughput::Bytes(layout.size() as u64));
        group.bench_with_input(BenchmarkId::new("Mnemosyne", name), &layout, |b, layout| {
            b.iter_batched(
                || unsafe {
                    require_allocated(mnemosyne::Mnemosyne.alloc(*layout), "dealloc_only")
                },
                |ptr| unsafe { dealloc_only(&mnemosyne::Mnemosyne, ptr, *layout) },
                BatchSize::SmallInput,
            );
        });
        group.bench_with_input(BenchmarkId::new("System", name), &layout, |b, layout| {
            b.iter_batched(
                || unsafe { require_allocated(System.alloc(*layout), "dealloc_only") },
                |ptr| unsafe { dealloc_only(&System, ptr, *layout) },
                BatchSize::SmallInput,
            )
        });
        group.bench_with_input(BenchmarkId::new("MiMalloc", name), &layout, |b, layout| {
            b.iter_batched(
                || unsafe { require_allocated(mimalloc::MiMalloc.alloc(*layout), "dealloc_only") },
                |ptr| unsafe { dealloc_only(&mimalloc::MiMalloc, ptr, *layout) },
                BatchSize::SmallInput,
            )
        });
        group.bench_with_input(BenchmarkId::new("RpMalloc", name), &layout, |b, layout| {
            b.iter_batched(
                || unsafe { require_allocated(rpmalloc::RpMalloc.alloc(*layout), "dealloc_only") },
                |ptr| unsafe { dealloc_only(&rpmalloc::RpMalloc, ptr, *layout) },
                BatchSize::SmallInput,
            )
        });
        group.bench_with_input(BenchmarkId::new("SnMalloc", name), &layout, |b, layout| {
            b.iter_batched(
                || unsafe {
                    require_allocated(snmalloc_rs::SnMalloc.alloc(*layout), "dealloc_only")
                },
                |ptr| unsafe { dealloc_only(&snmalloc_rs::SnMalloc, ptr, *layout) },
                BatchSize::SmallInput,
            )
        });
        #[cfg(jemalloc_available)]
        {
            group.bench_with_input(BenchmarkId::new("Jemalloc", name), &layout, |b, layout| {
                b.iter_batched(
                    || unsafe {
                        require_allocated(bench_jemalloc::Jemalloc.alloc(*layout), "dealloc_only")
                    },
                    |ptr| unsafe { dealloc_only(&bench_jemalloc::Jemalloc, ptr, *layout) },
                    BatchSize::SmallInput,
                )
            });
        }
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
        group.bench_with_input(BenchmarkId::new("System", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { burst_alloc_dealloc(&System, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("MiMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { burst_alloc_dealloc(&mimalloc::MiMalloc, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("RpMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { burst_alloc_dealloc(&rpmalloc::RpMalloc, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("SnMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { burst_alloc_dealloc(&snmalloc_rs::SnMalloc, *layout) })
        });
        #[cfg(jemalloc_available)]
        {
            group.bench_with_input(BenchmarkId::new("Jemalloc", name), &layout, |b, layout| {
                // Safety: `layout` comes from the static valid benchmark layout table.
                b.iter(|| unsafe { burst_alloc_dealloc(&bench_jemalloc::Jemalloc, *layout) })
            });
        }
    }
    group.finish();
}

fn bench_usable_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("Usable size latency");
    for (name, layout) in [
        ("small/32", SMALL_LAYOUT),
        ("medium/1024", MEDIUM_LAYOUT),
        ("large/8192", LARGE_LAYOUT),
        ("huge/2m", HUGE_LAYOUT),
    ] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::new("Mnemosyne", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe {
                alloc_usable_dealloc(&mnemosyne::Mnemosyne, *layout, |ptr| {
                    // Safety: `ptr` came from the Mnemosyne allocator above.
                    mnemosyne::usable_size(ptr)
                })
            })
        });
        group.bench_with_input(BenchmarkId::new("MiMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe {
                alloc_usable_dealloc(&mimalloc::MiMalloc, *layout, |ptr| {
                    // Safety: `ptr` came from the mimalloc allocator above.
                    mimalloc::MiMalloc.usable_size(ptr)
                })
            })
        });
        group.bench_with_input(BenchmarkId::new("SnMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe {
                alloc_usable_dealloc(&snmalloc_rs::SnMalloc, *layout, |ptr| {
                    match snmalloc_rs::SnMalloc.usable_size(ptr) {
                        Some(size) => size,
                        None => benchmark_failure("alloc_usable_dealloc", "snmalloc returned None"),
                    }
                })
            })
        });
        #[cfg(jemalloc_available)]
        {
            group.bench_with_input(BenchmarkId::new("Jemalloc", name), &layout, |b, layout| {
                // Safety: `layout` comes from the static valid benchmark layout table.
                b.iter(|| unsafe {
                    alloc_usable_dealloc(&bench_jemalloc::Jemalloc, *layout, |ptr| {
                        // Safety: `ptr` came from the jemalloc allocator above;
                        // the call is covered by the enclosing `unsafe` block.
                        bench_jemalloc::usable_size(ptr)
                    })
                })
            });
        }
    }
    group.finish();
}

fn bench_usable_size_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("Usable size query latency");
    for (name, layout) in [
        ("small/32", SMALL_LAYOUT),
        ("medium/1024", MEDIUM_LAYOUT),
        ("large/8192", LARGE_LAYOUT),
        ("huge/2m", HUGE_LAYOUT),
    ] {
        group.throughput(Throughput::Elements(1));

        // Safety: `layout` comes from the static valid benchmark layout table.
        let mnemosyne_ptr =
            unsafe { require_allocated(mnemosyne::Mnemosyne.alloc(layout), "usable_size_query") };
        group.bench_with_input(
            BenchmarkId::new("Mnemosyne", name),
            &mnemosyne_ptr,
            |b, ptr| b.iter(|| unsafe { mnemosyne::usable_size(black_box(*ptr)) }),
        );
        // Safety: pointer was allocated by Mnemosyne for `layout` above.
        unsafe { mnemosyne::Mnemosyne.dealloc(mnemosyne_ptr, layout) };

        // Safety: `layout` comes from the static valid benchmark layout table.
        let mimalloc_ptr =
            unsafe { require_allocated(mimalloc::MiMalloc.alloc(layout), "usable_size_query") };
        group.bench_with_input(
            BenchmarkId::new("MiMalloc", name),
            &mimalloc_ptr,
            |b, ptr| b.iter(|| unsafe { mimalloc::MiMalloc.usable_size(black_box(*ptr)) }),
        );
        // Safety: pointer was allocated by MiMalloc for `layout` above.
        unsafe { mimalloc::MiMalloc.dealloc(mimalloc_ptr, layout) };

        // Safety: `layout` comes from the static valid benchmark layout table.
        let snmalloc_ptr =
            unsafe { require_allocated(snmalloc_rs::SnMalloc.alloc(layout), "usable_size_query") };
        group.bench_with_input(
            BenchmarkId::new("SnMalloc", name),
            &snmalloc_ptr,
            |b, ptr| {
                b.iter(
                    || match snmalloc_rs::SnMalloc.usable_size(black_box(*ptr)) {
                        Some(size) => size,
                        None => benchmark_failure("usable_size_query", "snmalloc returned None"),
                    },
                )
            },
        );
        // Safety: pointer was allocated by SnMalloc for `layout` above.
        unsafe { snmalloc_rs::SnMalloc.dealloc(snmalloc_ptr, layout) };

        #[cfg(jemalloc_available)]
        {
            // Safety: `layout` comes from the static valid benchmark layout table.
            let jemalloc_ptr = unsafe {
                require_allocated(bench_jemalloc::Jemalloc.alloc(layout), "usable_size_query")
            };
            group.bench_with_input(
                BenchmarkId::new("Jemalloc", name),
                &jemalloc_ptr,
                |b, ptr| b.iter(|| unsafe { bench_jemalloc::usable_size(black_box(*ptr)) }),
            );
            // Safety: pointer was allocated by Jemalloc for `layout` above.
            unsafe { bench_jemalloc::Jemalloc.dealloc(jemalloc_ptr, layout) };
        }
    }
    group.finish();
}

fn bench_realloc(c: &mut Criterion) {
    let mut group = c.benchmark_group("Realloc latency");
    for (name, layout, new_size) in [
        ("within_class_24_to_32", SMALL_WITHIN_CLASS_LAYOUT, 32usize),
        ("cross_class_32_to_64", SMALL_LAYOUT, 64usize),
        (
            "within_class_6k_to_8k",
            LARGE_WITHIN_CLASS_LAYOUT,
            8192usize,
        ),
        ("cross_class_8k_to_16k", LARGE_LAYOUT, 16384usize),
        (
            "huge_shrink_4m_to_2m",
            HUGE_REALLOC_SRC_LAYOUT,
            2 * 1024 * 1024usize,
        ),
    ] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("Mnemosyne", name),
            &(layout, new_size),
            |b, (layout, new_size)| {
                // Safety: inputs come from the static valid benchmark layout table.
                b.iter(|| unsafe {
                    alloc_realloc_dealloc(&mnemosyne::Mnemosyne, *layout, *new_size)
                })
            },
        );
        group.bench_with_input(
            BenchmarkId::new("System", name),
            &(layout, new_size),
            |b, (layout, new_size)| {
                // Safety: inputs come from the static valid benchmark layout table.
                b.iter(|| unsafe { alloc_realloc_dealloc(&System, *layout, *new_size) })
            },
        );
        group.bench_with_input(
            BenchmarkId::new("MiMalloc", name),
            &(layout, new_size),
            |b, (layout, new_size)| {
                // Safety: inputs come from the static valid benchmark layout table.
                b.iter(|| unsafe { alloc_realloc_dealloc(&mimalloc::MiMalloc, *layout, *new_size) })
            },
        );
        group.bench_with_input(
            BenchmarkId::new("RpMalloc", name),
            &(layout, new_size),
            |b, (layout, new_size)| {
                // Safety: inputs come from the static valid benchmark layout table.
                b.iter(|| unsafe { alloc_realloc_dealloc(&rpmalloc::RpMalloc, *layout, *new_size) })
            },
        );
        group.bench_with_input(
            BenchmarkId::new("SnMalloc", name),
            &(layout, new_size),
            |b, (layout, new_size)| {
                // Safety: inputs come from the static valid benchmark layout table.
                b.iter(|| unsafe {
                    alloc_realloc_dealloc(&snmalloc_rs::SnMalloc, *layout, *new_size)
                })
            },
        );
        #[cfg(jemalloc_available)]
        {
            group.bench_with_input(
                BenchmarkId::new("Jemalloc", name),
                &(layout, new_size),
                |b, (layout, new_size)| {
                    // Safety: inputs come from the static valid benchmark layout table.
                    b.iter(|| unsafe {
                        alloc_realloc_dealloc(&bench_jemalloc::Jemalloc, *layout, *new_size)
                    })
                },
            );
        }
    }
    group.finish();
}

fn bench_cross_thread_free(c: &mut Criterion) {
    static MNEMOSYNE: mnemosyne::Mnemosyne = mnemosyne::Mnemosyne;
    static SYSTEM: System = System;
    static MIMALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;
    static RPMALLOC: rpmalloc::RpMalloc = rpmalloc::RpMalloc;
    static SNMALLOC: snmalloc_rs::SnMalloc = snmalloc_rs::SnMalloc;
    #[cfg(jemalloc_available)]
    static JEMALLOC: bench_jemalloc::Jemalloc = bench_jemalloc::Jemalloc;

    let mut group = c.benchmark_group("Cross-thread free handoff");
    for (name, layout) in [
        ("small/32", SMALL_LAYOUT),
        ("medium/1024", MEDIUM_LAYOUT),
        ("large/8192", LARGE_LAYOUT),
        ("huge/2m", HUGE_LAYOUT),
    ] {
        let count = if layout.size() > 64 * 1024 {
            8 // Avoid high memory pressure for huge allocations
        } else {
            CROSS_THREAD_ALLOCS
        };
        group.throughput(Throughput::Elements(count as u64));
        let mnemosyne_worker = HandoffWorker::new(&MNEMOSYNE);
        group.bench_with_input(BenchmarkId::new("Mnemosyne", name), &layout, |b, layout| {
            b.iter(|| mnemosyne_worker.alloc_then_handoff(*layout, count))
        });
        drop(mnemosyne_worker);

        let system_worker = HandoffWorker::new(&SYSTEM);
        group.bench_with_input(BenchmarkId::new("System", name), &layout, |b, layout| {
            b.iter(|| system_worker.alloc_then_handoff(*layout, count))
        });
        drop(system_worker);

        let mimalloc_worker = HandoffWorker::new(&MIMALLOC);
        group.bench_with_input(BenchmarkId::new("MiMalloc", name), &layout, |b, layout| {
            b.iter(|| mimalloc_worker.alloc_then_handoff(*layout, count))
        });
        drop(mimalloc_worker);

        let rpmalloc_worker = HandoffWorker::new(&RPMALLOC);
        group.bench_with_input(BenchmarkId::new("RpMalloc", name), &layout, |b, layout| {
            b.iter(|| rpmalloc_worker.alloc_then_handoff(*layout, count))
        });
        drop(rpmalloc_worker);

        let snmalloc_worker = HandoffWorker::new(&SNMALLOC);
        group.bench_with_input(BenchmarkId::new("SnMalloc", name), &layout, |b, layout| {
            b.iter(|| snmalloc_worker.alloc_then_handoff(*layout, count))
        });
        drop(snmalloc_worker);

        #[cfg(jemalloc_available)]
        {
            let jemalloc_worker = HandoffWorker::new(&JEMALLOC);
            group.bench_with_input(BenchmarkId::new("Jemalloc", name), &layout, |b, layout| {
                b.iter(|| jemalloc_worker.alloc_then_handoff(*layout, count))
            });
            drop(jemalloc_worker);
        }
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
    static SYSTEM: System = System;
    static MIMALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;
    static RPMALLOC: rpmalloc::RpMalloc = rpmalloc::RpMalloc;
    static SNMALLOC: snmalloc_rs::SnMalloc = snmalloc_rs::SnMalloc;
    #[cfg(jemalloc_available)]
    static JEMALLOC: bench_jemalloc::Jemalloc = bench_jemalloc::Jemalloc;

    {
        let mut group = c.benchmark_group("Threaded small allocation cycles");
        group.throughput(Throughput::Elements((THREADS * THREAD_ALLOCS) as u64));

        let mnemosyne_workers = ThreadCycleWorkers::new(&MNEMOSYNE, SMALL_LAYOUT);
        group.bench_function("Mnemosyne", |b| b.iter(|| mnemosyne_workers.run()));
        drop(mnemosyne_workers);

        let system_workers = ThreadCycleWorkers::new(&SYSTEM, SMALL_LAYOUT);
        group.bench_function("System", |b| b.iter(|| system_workers.run()));
        drop(system_workers);

        let mimalloc_workers = ThreadCycleWorkers::new(&MIMALLOC, SMALL_LAYOUT);
        group.bench_function("MiMalloc", |b| b.iter(|| mimalloc_workers.run()));
        drop(mimalloc_workers);

        let rpmalloc_workers = ThreadCycleWorkers::new(&RPMALLOC, SMALL_LAYOUT);
        group.bench_function("RpMalloc", |b| b.iter(|| rpmalloc_workers.run()));
        drop(rpmalloc_workers);

        let snmalloc_workers = ThreadCycleWorkers::new(&SNMALLOC, SMALL_LAYOUT);
        group.bench_function("SnMalloc", |b| b.iter(|| snmalloc_workers.run()));
        drop(snmalloc_workers);

        #[cfg(jemalloc_available)]
        {
            let jemalloc_workers = ThreadCycleWorkers::new(&JEMALLOC, SMALL_LAYOUT);
            group.bench_function("Jemalloc", |b| b.iter(|| jemalloc_workers.run()));
            drop(jemalloc_workers);
        }
        group.finish();
    }

    {
        let mut group = c.benchmark_group("Threaded medium allocation cycles");
        group.throughput(Throughput::Elements((THREADS * THREAD_ALLOCS) as u64));

        let mnemosyne_workers = ThreadCycleWorkers::new(&MNEMOSYNE, MEDIUM_LAYOUT);
        group.bench_function("Mnemosyne", |b| b.iter(|| mnemosyne_workers.run()));
        drop(mnemosyne_workers);

        let system_workers = ThreadCycleWorkers::new(&SYSTEM, MEDIUM_LAYOUT);
        group.bench_function("System", |b| b.iter(|| system_workers.run()));
        drop(system_workers);

        let mimalloc_workers = ThreadCycleWorkers::new(&MIMALLOC, MEDIUM_LAYOUT);
        group.bench_function("MiMalloc", |b| b.iter(|| mimalloc_workers.run()));
        drop(mimalloc_workers);

        let rpmalloc_workers = ThreadCycleWorkers::new(&RPMALLOC, MEDIUM_LAYOUT);
        group.bench_function("RpMalloc", |b| b.iter(|| rpmalloc_workers.run()));
        drop(rpmalloc_workers);

        let snmalloc_workers = ThreadCycleWorkers::new(&SNMALLOC, MEDIUM_LAYOUT);
        group.bench_function("SnMalloc", |b| b.iter(|| snmalloc_workers.run()));
        drop(snmalloc_workers);

        #[cfg(jemalloc_available)]
        {
            let jemalloc_workers = ThreadCycleWorkers::new(&JEMALLOC, MEDIUM_LAYOUT);
            group.bench_function("Jemalloc", |b| b.iter(|| jemalloc_workers.run()));
            drop(jemalloc_workers);
        }
        group.finish();
    }
}

fn bench_saturated_multithreaded_alloc(c: &mut Criterion) {
    static MNEMOSYNE: mnemosyne::Mnemosyne = mnemosyne::Mnemosyne;
    static SYSTEM: System = System;
    static MIMALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;
    static RPMALLOC: rpmalloc::RpMalloc = rpmalloc::RpMalloc;
    static SNMALLOC: snmalloc_rs::SnMalloc = snmalloc_rs::SnMalloc;
    #[cfg(jemalloc_available)]
    static JEMALLOC: bench_jemalloc::Jemalloc = bench_jemalloc::Jemalloc;

    let mut group = c.benchmark_group("Threaded saturated small allocation cycles");
    group.throughput(Throughput::Elements(
        (THREADS * SATURATED_THREAD_ALLOCS) as u64,
    ));

    let mnemosyne_workers = ThreadCycleWorkers::new(&MNEMOSYNE, SMALL_LAYOUT);
    group.bench_function("Mnemosyne", |b| {
        b.iter(|| mnemosyne_workers.run_with_iterations(SATURATED_THREAD_ALLOCS))
    });
    drop(mnemosyne_workers);

    let system_workers = ThreadCycleWorkers::new(&SYSTEM, SMALL_LAYOUT);
    group.bench_function("System", |b| {
        b.iter(|| system_workers.run_with_iterations(SATURATED_THREAD_ALLOCS))
    });
    drop(system_workers);

    let mimalloc_workers = ThreadCycleWorkers::new(&MIMALLOC, SMALL_LAYOUT);
    group.bench_function("MiMalloc", |b| {
        b.iter(|| mimalloc_workers.run_with_iterations(SATURATED_THREAD_ALLOCS))
    });
    drop(mimalloc_workers);

    let rpmalloc_workers = ThreadCycleWorkers::new(&RPMALLOC, SMALL_LAYOUT);
    group.bench_function("RpMalloc", |b| {
        b.iter(|| rpmalloc_workers.run_with_iterations(SATURATED_THREAD_ALLOCS))
    });
    drop(rpmalloc_workers);

    let snmalloc_workers = ThreadCycleWorkers::new(&SNMALLOC, SMALL_LAYOUT);
    group.bench_function("SnMalloc", |b| {
        b.iter(|| snmalloc_workers.run_with_iterations(SATURATED_THREAD_ALLOCS))
    });
    drop(snmalloc_workers);

    #[cfg(jemalloc_available)]
    {
        let jemalloc_workers = ThreadCycleWorkers::new(&JEMALLOC, SMALL_LAYOUT);
        group.bench_function("Jemalloc", |b| {
            b.iter(|| jemalloc_workers.run_with_iterations(SATURATED_THREAD_ALLOCS))
        });
        drop(jemalloc_workers);
    }

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

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(100))
        .measurement_time(Duration::from_millis(500));
    targets =
        bench_allocator_cycles,
        bench_allocator_alloc,
        bench_allocator_dealloc,
        bench_allocator_bursts,
        bench_usable_size,
        bench_usable_size_query,
        bench_realloc,
        bench_cross_thread_free,
        bench_multithreaded_alloc,
        bench_saturated_multithreaded_alloc,
        bench_segment_cache_eviction
}
criterion_main!(benches);
