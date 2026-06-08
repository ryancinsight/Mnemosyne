use core::alloc::Layout;

// Safety: all benchmark layouts use nonzero power-of-two alignments and fixed
// positive sizes.
pub const SMALL_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(32, 8) };
pub const SMALL_WITHIN_CLASS_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(24, 8) };
pub const MEDIUM_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(1024, 8) };
pub const LARGE_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(8192, 8) };
pub const LARGE_WITHIN_CLASS_LAYOUT: Layout =
    unsafe { Layout::from_size_align_unchecked(6144, 8) };
pub const HUGE_LAYOUT: Layout =
    unsafe { Layout::from_size_align_unchecked(2 * 1024 * 1024, 4096) };
pub const HUGE_REALLOC_SRC_LAYOUT: Layout =
    unsafe { Layout::from_size_align_unchecked(4 * 1024 * 1024, 4096) };
pub const BATCH_ALLOCS: usize = 256;
pub const THREADS: usize = 4;
pub const THREAD_ALLOCS: usize = 1_000;
pub const SATURATED_THREAD_ALLOCS: usize = 16_000;
pub const CROSS_THREAD_ALLOCS: usize = 512;
pub const CROSS_THREAD_QUEUE_BOUND: usize = 2;
pub const THREAD_WORK_QUEUE_BOUND: usize = THREADS;
pub const SEGMENT_EVICTION_ALLOCS: usize = mnemosyne_arena::MAX_RETAINED_SEGMENTS + 8;
