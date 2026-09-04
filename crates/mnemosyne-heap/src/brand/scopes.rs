use crate::Heap;
use crate::brand::{SyncRegionToken, ThreadLocalToken};
use crate::raw_heap::RawHeap;
use core::marker::PhantomData;
use melinoe::sync::{sync_region_scope, thread_local_scope};
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::LocalAllocatorSelector;
use mnemosyne_local::internal::HasSegmentPool;

/// Executes a closure with a fresh, compile-time unique branded heap and token.
///
/// # Examples
///
/// ```
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper;
/// use mnemosyne_heap::scope;
///
/// scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
///     let val = mnemosyne_heap::BrandedBox::new(&heap, &token, 42)
///         .expect("branded box allocation failed");
///     assert_eq!(*val, 42);
/// });
/// ```
///
/// This example fails to compile because it attempts to escape a branded block from its scope:
///
/// ```compile_fail
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper;
/// use mnemosyne_heap::{scope, BrandedBlock};
///
/// let mut escaped: Option<BrandedBlock<'static, i32>> = None;
/// scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
///     let block = heap.alloc_init(&token, 42)
///         .expect("branded block allocation failed");
///     // This compile error is expected because the 'brand lifetime cannot escape the closure scope:
///     escaped = Some(block);
/// });
/// ```
///
/// Proving that thread-exclusivity bounds are enforced at compile time.
/// Since the melinoe `ThreadLocalToken` is `!Send` and `!Sync`, the following fails to compile:
///
/// ```compile_fail
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper;
/// use mnemosyne_heap::scope;
/// use std::thread;
///
/// scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
///     // ThreadLocalToken is !Send, so sending it to another thread is a compile error:
///     thread::spawn(move || {
///         let _t = token;
///     });
/// });
/// ```
///
/// Proving that `BrandedBox` is `!Send`:
///
/// ```compile_fail
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper;
/// use mnemosyne_heap::scope;
/// use std::thread;
///
/// scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
///     let val = heap.alloc_init(&token, 42)
///         .expect("branded box send-bound allocation failed");
///     // SAFETY: `val` was just allocated through `heap` and is exclusively
///     // owned; `BrandedBox::from_raw` takes the allocation back.
///     let boxed = unsafe { mnemosyne_heap::BrandedBox::from_raw(&heap, val) };
///     // BrandedBox is !Send, so sending it to another thread is a compile error:
///     thread::spawn(move || {
///         let _b = boxed;
///     });
/// });
/// ```
///
/// Proving that `BrandedVec` is `!Send`:
///
/// ```compile_fail
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper;
/// use mnemosyne_heap::{scope, BrandedVec};
/// use std::thread;
///
/// scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
///     let mut vec = BrandedVec::new(&heap);
///     // BrandedVec is !Send, so sending it to another thread is a compile error:
///     thread::spawn(move || {
///         let _v = vec;
///     });
/// });
/// ```
///
/// Proving that two distinct scopes cannot mix allocation tokens or heaps:
///
/// ```compile_fail
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper;
/// use mnemosyne_heap::scope;
///
/// scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap1, mut token1| {
///     scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap2, mut token2| {
///         let val = heap1.alloc_init(&token1, 42)
///             .expect("cross-scope branded allocation failed");
///         // This fails to compile because token2 has a different 'brand:
///         heap2.free(&mut token2, val);
///     });
/// });
/// ```
pub fn scope<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>, F, R>(f: F) -> R
where
    F: for<'brand> FnOnce(Heap<'brand, P, B>, ThreadLocalToken<'brand>) -> R,
{
    // The brand identity, uniqueness, and thread-confined capability token are
    // minted by melinoe. The higher-ranked `'brand` from `thread_local_scope`
    // is shared with the `Heap` constructed under it, so the heap and its token
    // are provably the only pair for this brand and cannot escape the closure.
    thread_local_scope(|token| {
        let heap = Heap {
            raw: RawHeap::new(),
            _phantom: PhantomData,
        };
        f(heap, token)
    })
}

/// Executes a closure with a fresh, thread-portable branded heap and token.
///
/// The heap remains one exclusive allocator owner. A caller may move
/// [`crate::BrandedCell`] handles and [`SyncRegionToken`] into scoped workers,
/// then return them to the owner before reclaiming their blocks through the
/// heap. Payload access is mediated by Melinoe permits and does not allocate or
/// synchronize at runtime.
///
/// # Examples
///
/// ```
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper;
/// use mnemosyne_heap::{sync_scope, BrandedCell};
///
/// sync_scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
///     let block = heap.alloc_init(&token, 41).expect("allocation failed");
///     // SAFETY: `alloc_init` returned an initialized block owned by `heap`.
///     let cell = unsafe { BrandedCell::from_block(block) };
///     let (cell, mut token) = std::thread::scope(|scope| {
///         scope
///             .spawn(move || {
///                 *cell.borrow_mut(&mut token) += 1;
///                 (cell, token)
///             })
///             .join()
///             .expect("scoped worker panicked")
///     });
///     assert_eq!(*cell.borrow(&token), 42);
///     heap.free(&mut token, unsafe { cell.into_block() });
/// });
/// ```
#[inline]
pub fn sync_scope<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>, F, R>(f: F) -> R
where
    F: for<'brand> FnOnce(Heap<'brand, P, B>, SyncRegionToken<'brand>) -> R,
{
    sync_region_scope(|token| {
        let heap = Heap {
            raw: RawHeap::new(),
            _phantom: PhantomData,
        };
        f(heap, token)
    })
}
