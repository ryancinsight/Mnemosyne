use core::alloc::{GlobalAlloc, Layout};
use criterion::black_box;

use super::constants::BATCH_ALLOCS;

#[cold]
pub fn benchmark_failure(context: &str, detail: &str) -> ! {
    eprintln!("benchmark failure: {context}: {detail}");
    std::process::exit(2);
}

#[inline(always)]
pub fn require_allocated(ptr: *mut u8, context: &str) -> *mut u8 {
    if ptr.is_null() {
        benchmark_failure(context, "allocator returned a null pointer");
    }
    ptr
}

pub struct AllocatedBlock<'a, A: GlobalAlloc> {
    pub allocator: &'a A,
    pub ptr: *mut u8,
    pub layout: Layout,
}

impl<'a, A: GlobalAlloc> AllocatedBlock<'a, A> {
    #[inline(always)]
    pub unsafe fn new(allocator: &'a A, layout: Layout, context: &str) -> Self {
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
pub unsafe fn alloc_dealloc<A: GlobalAlloc>(allocator: &A, layout: Layout) {
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
pub unsafe fn dealloc_only<A: GlobalAlloc>(allocator: &A, ptr: *mut u8, layout: Layout) {
    allocator.dealloc(black_box(ptr), layout);
}

#[inline(never)]
/// Allocates and deallocates a fixed batch for burst-retention benchmarks.
///
/// # Safety
///
/// `layout` must be a valid layout for `allocator`, and each allocation is
/// deallocated exactly once by this function.
pub unsafe fn burst_alloc_dealloc<A: GlobalAlloc>(allocator: &A, layout: Layout) {
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
pub unsafe fn alloc_usable_dealloc<A, F>(allocator: &A, layout: Layout, usable_size: F)
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
pub unsafe fn alloc_realloc_dealloc<A: GlobalAlloc>(
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
