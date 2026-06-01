#![allow(clippy::missing_const_for_thread_local)]
extern crate std;
use super::*;
use core::alloc::Layout;
use mnemosyne_backend::MemoryBackendWrapper;
use mnemosyne_core::StandardPolicy;
use std::format;

fn test_layout(size: usize, align: usize) -> Layout {
    Layout::from_size_align(size, align)
        .expect("heap unit test layout must use a nonzero power-of-two alignment")
}

#[test]
fn test_heap_allocation_and_free() {
    let heap = MnemosyneHeap::<StandardPolicy, MemoryBackendWrapper>::new();
    let layout = test_layout(32, 8);
    let ptr = heap.alloc(layout);
    assert!(!ptr.is_null(), "heap allocation failed");

    unsafe {
        ptr.write(42);
        assert_eq!(ptr.read(), 42);
        heap.free(ptr);
    }
}

#[test]
fn test_heap_realloc() {
    let heap = MnemosyneHeap::<StandardPolicy, MemoryBackendWrapper>::new();
    let layout = test_layout(16, 8);
    let ptr = heap.alloc(layout);
    assert!(!ptr.is_null());

    unsafe {
        ptr.write(99);
        let new_ptr = heap.realloc(ptr, layout, 32);
        assert!(!new_ptr.is_null());
        assert_eq!(new_ptr.read(), 99);
        heap.free(new_ptr);
    }
}

#[test]
fn test_branded_heap_allocation_and_free() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let layout = test_layout(32, 8);
        let block = heap
            .alloc(&token, layout)
            .expect("branded allocation failed");
        let ptr = block.as_ptr();
        assert!(!ptr.is_null());
        unsafe {
            ptr.write(42);
            assert_eq!(ptr.read(), 42);
        }
        heap.free(&mut token, block);
    });
}

#[test]
fn test_branded_heap_realloc() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let layout = test_layout(16, 8);
        let block = heap
            .alloc(&token, layout)
            .expect("branded allocation failed");
        let ptr = block.as_ptr();
        unsafe {
            ptr.write(99);
        }
        let new_block = heap
            .realloc(&mut token, block, layout, 32)
            .expect("branded realloc failed");
        let new_ptr = new_block.as_ptr();
        unsafe {
            assert_eq!(new_ptr.read(), 99);
        }
        heap.free(&mut token, new_block);
    });
}

#[test]
fn test_branded_heap_realloc_zst_to_nonzero_skips_source_free() {
    struct Marker;

    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let before = heap.stats();
        let block = heap.alloc_init(&token, Marker).expect("ZST alloc failed");
        let after_zst_alloc = heap.stats();

        assert_eq!(
            after_zst_alloc.current_thread_live_allocations, before.current_thread_live_allocations,
            "ZST source construction must not create a live allocator block"
        );

        let new_block = heap
            .realloc(&mut token, block, Layout::new::<Marker>(), 16)
            .expect("ZST-to-nonzero realloc failed");
        let after_alloc = heap.stats();

        assert!(
            !new_block.as_ptr().is_null(),
            "realloc returned a null block"
        );
        assert!(
            after_alloc.current_thread_live_allocations
                > after_zst_alloc.current_thread_live_allocations,
            "nonzero destination must create a live allocator block"
        );

        heap.free_uninit(&mut token, new_block);
        let after_free = heap.stats();
        assert!(
            after_free.current_thread_live_allocations
                < after_alloc.current_thread_live_allocations,
            "free_uninit must release the nonzero destination block"
        );
    });
}

#[test]
fn test_branded_heap_realloc_zst_to_zero_drops_without_allocating() {
    ZST_DROP_COUNT.with(|c| c.set(0));
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let before = heap.stats();
        let block = heap
            .alloc_init(&token, ZstDrop)
            .expect("ZST alloc_init failed");

        let result = heap.realloc(&mut token, block, Layout::new::<ZstDrop>(), 0);
        let after = heap.stats();

        assert!(
            result.is_none(),
            "ZST-to-zero realloc must consume the block without a replacement"
        );
        assert_eq!(
            after.current_thread_live_allocations, before.current_thread_live_allocations,
            "ZST-to-zero realloc must not allocate or free a real block"
        );
        assert_eq!(
            ZST_DROP_COUNT.with(|c| c.get()),
            1,
            "ZST-to-zero realloc must drop the owned value exactly once"
        );
    });
}

#[test]
fn test_branded_heap_generic_and_cast() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let layout = test_layout(32, 8);
        let block: BrandedBlock<'_, u8> = heap
            .alloc(&token, layout)
            .expect("branded allocation failed");

        // Cast to i32 block
        let casted: BrandedBlock<'_, i32> = block.cast::<i32>();
        let ptr = casted.as_ptr();
        assert!(!ptr.is_null());
        unsafe {
            ptr.write(123456);
            assert_eq!(ptr.read(), 123456);
        }

        // Free generic block
        heap.free(&mut token, casted);
    });
}

use core::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
struct DropTracker<'a>(&'a AtomicUsize);
impl<'a> Drop for DropTracker<'a> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn test_branded_box_and_drop_tracking() {
    let counter = AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
        let bbox = BrandedBox::new(&heap, &token, DropTracker(&counter))
            .expect("BrandedBox allocation failed");
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        // Drop should occur here when bbox goes out of scope.
        drop(bbox);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn test_branded_heap_free_drops_value() {
    let counter = AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let block = heap
            .alloc_init(&token, DropTracker(&counter))
            .expect("alloc_init failed");
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        heap.free(&mut token, block);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn test_branded_heap_alloc_init_zst_drops_without_allocating() {
    ZST_DROP_COUNT.with(|c| c.set(0));
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let before = heap.stats().current_thread_owned_segments;
        let block = heap
            .alloc_init(&token, ZstDrop)
            .expect("ZST alloc_init failed");
        assert_eq!(
            heap.stats().current_thread_owned_segments,
            before,
            "ZST alloc_init must not allocate a segment"
        );
        heap.free(&mut token, block);
        assert_eq!(ZST_DROP_COUNT.with(|c| c.get()), 1);
    });
}

#[test]
fn test_branded_vec_growth_and_drop() {
    let counter = AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let mut vec = BrandedVec::new(&heap);
        assert!(vec.is_empty());
        assert_eq!(vec.len(), 0);
        assert_eq!(vec.capacity(), 0);

        // Push elements to trigger growth
        for _ in 0..10 {
            vec.push(&mut token, DropTracker(&counter)).unwrap();
        }
        assert_eq!(vec.len(), 10);
        assert!(vec.capacity() >= 10);

        // Pop half of the elements
        for _ in 0..5 {
            let popped = vec.pop();
            assert!(popped.is_some());
            drop(popped);
        }
        assert_eq!(vec.len(), 5);
        assert_eq!(counter.load(Ordering::SeqCst), 5); // 5 popped elements dropped

        // Drop vec, remainder should drop
        drop(vec);
        assert_eq!(counter.load(Ordering::SeqCst), 10); // all 10 elements dropped
    });
}

std::thread_local! {
    static ZST_DROP_COUNT: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

#[derive(Debug)]
struct ZstDrop;

impl Drop for ZstDrop {
    fn drop(&mut self) {
        ZST_DROP_COUNT.with(|c| c.set(c.get() + 1));
    }
}

#[test]
fn test_branded_box_zst_drops_without_allocating() {
    ZST_DROP_COUNT.with(|c| c.set(0));
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
        let before = heap.stats().current_thread_owned_segments;
        let bbox = BrandedBox::new(&heap, &token, ZstDrop).expect("ZST box allocation failed");
        let after_new = heap.stats().current_thread_owned_segments;
        assert_eq!(after_new, before, "ZST box must not allocate a segment");
        drop(bbox);
        assert_eq!(ZST_DROP_COUNT.with(|c| c.get()), 1);
    });
}

#[test]
fn test_branded_vec_zst_uses_sentinel_capacity_and_drops_elements() {
    ZST_DROP_COUNT.with(|c| c.set(0));
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let before = heap.stats().current_thread_owned_segments;
        let mut vec =
            BrandedVec::with_capacity(&heap, &token, 8).expect("ZST vector construction failed");
        assert_eq!(vec.capacity(), usize::MAX);
        assert_eq!(
            heap.stats().current_thread_owned_segments,
            before,
            "ZST vector capacity must not allocate a segment"
        );

        for _ in 0..4 {
            vec.push(&mut token, ZstDrop).expect("ZST push failed");
        }
        assert_eq!(vec.len(), 4);
        drop(vec.pop());
        assert_eq!(ZST_DROP_COUNT.with(|c| c.get()), 1);
        drop(vec);
        assert_eq!(ZST_DROP_COUNT.with(|c| c.get()), 4);
    });
}

#[test]
fn test_branded_vec_new_zst_preserves_capacity_invariant() {
    ZST_DROP_COUNT.with(|c| c.set(0));
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let before = heap.stats().current_thread_owned_segments;
        let mut vec = BrandedVec::new(&heap);

        assert_eq!(
            vec.capacity(),
            usize::MAX,
            "ZST vector constructed with new must use sentinel capacity"
        );
        vec.push(&mut token, ZstDrop).expect("ZST push failed");
        assert_eq!(vec.len(), 1);
        assert!(
            vec.len() <= vec.capacity(),
            "successful push must preserve len <= capacity"
        );
        assert_eq!(
            heap.stats().current_thread_owned_segments,
            before,
            "ZST vector constructed with new must not allocate a segment"
        );

        drop(vec);
        assert_eq!(ZST_DROP_COUNT.with(|c| c.get()), 1);
    });
}

#[test]
fn test_branded_cell_sharing_and_mutation() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let block = heap.alloc_init(&token, 42).expect("alloc_init failed");
        let cell = unsafe { BrandedCell::from_block(block) };

        // Cell is Copy/Clone, create multiple copies
        let cell_copy1 = cell;
        let cell_copy2 = cell;

        // Read original value
        assert_eq!(*cell_copy1.borrow(&token), 42);
        assert_eq!(*cell_copy2.borrow(&token), 42);

        // Mutate value via mutable borrow
        *cell_copy1.borrow_mut(&mut token) = 100;

        // Verify mutation is reflected in all copies
        assert_eq!(*cell_copy2.borrow(&token), 100);
        assert_eq!(*cell.borrow(&token), 100);

        // Free the memory using the safe/encapsulated conversion
        heap.free(&mut token, unsafe { cell.into_block() });
    });
}

#[test]
fn test_branded_box_unsized_slice_and_drop() {
    let counter = AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let mut vec = BrandedVec::new(&heap);
        for _ in 0..5 {
            vec.push(&mut token, DropTracker(&counter)).unwrap();
        }
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Convert to boxed slice
        let boxed_slice = vec.into_boxed_slice(&mut token);
        assert_eq!(boxed_slice.len(), 5);
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Drop boxed slice
        drop(boxed_slice);
        assert_eq!(counter.load(Ordering::SeqCst), 5); // All 5 elements dropped
    });
}

#[test]
fn test_branded_vec_into_boxed_slice_shrinks_storage_to_len() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let mut vec = BrandedVec::with_capacity(&heap, &token, 1024)
            .expect("oversized vector allocation failed");
        vec.push(&mut token, 0xCAFE_BABEu64)
            .expect("push into preallocated vector failed");

        let before_usable = unsafe { mnemosyne_local::usable_size(vec.as_ptr() as *mut u8) };
        let boxed_slice = vec.into_boxed_slice(&mut token);
        let after_usable = unsafe { mnemosyne_local::usable_size(boxed_slice.as_ptr() as *mut u8) };

        assert_eq!(boxed_slice.len(), 1);
        assert_eq!(boxed_slice[0], 0xCAFE_BABE);
        assert!(
            after_usable <= before_usable,
            "boxed slice conversion must not increase usable storage from {before_usable}, got {after_usable}"
        );
    });
}

#[test]
fn test_branded_box_into_and_from_raw() {
    let counter = AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
        let bbox = BrandedBox::new(&heap, &token, DropTracker(&counter))
            .expect("BrandedBox allocation failed");
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Convert to raw block
        let block = bbox.into_raw();
        assert_eq!(counter.load(Ordering::SeqCst), 0); // No drop yet

        // Reconstruct BrandedBox from raw block
        let bbox_reconstructed = unsafe { BrandedBox::from_raw(&heap, block) };
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Drop reconstructed box
        drop(bbox_reconstructed);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn test_branded_box_into_cell() {
    let counter = AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let bbox = BrandedBox::new(&heap, &token, DropTracker(&counter))
            .expect("BrandedBox allocation failed");
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Convert to shared BrandedCell
        let cell = bbox.into_cell();
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Read the cell
        assert_eq!(cell.borrow(&token).0.load(Ordering::SeqCst), 0);

        // Reclaim memory using the safe/encapsulated conversion
        heap.free(&mut token, unsafe { cell.into_block() });
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn test_branded_cell_unsized_slice() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let mut vec = BrandedVec::new(&heap);
        vec.push(&mut token, 10).unwrap();
        vec.push(&mut token, 20).unwrap();
        vec.push(&mut token, 30).unwrap();

        let boxed_slice = vec.into_boxed_slice(&mut token);
        assert_eq!(boxed_slice.len(), 3);

        let cell = boxed_slice.into_cell();
        assert_eq!(cell.borrow(&token), &[10, 20, 30]);

        // Mutate cell slice elements
        cell.borrow_mut(&mut token)[1] = 99;
        assert_eq!(cell.borrow(&token), &[10, 99, 30]);

        heap.free(&mut token, unsafe { cell.into_block() });
    });
}

#[test]
fn test_branded_cell_multi_mutable_borrow() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let b1 = heap.alloc_init(&token, 1).unwrap();
        let b2 = heap.alloc_init(&token, 2.0).unwrap();
        let b3 = heap
            .alloc_init(&token, std::string::String::from("3"))
            .unwrap();

        let c1 = unsafe { BrandedCell::from_block(b1) };
        let c2 = unsafe { BrandedCell::from_block(b2) };
        let c3 = unsafe { BrandedCell::from_block(b3) };

        {
            let (r1, r2, r3) = BrandedCell::borrow_mut_3(&c1, &c2, &c3, &mut token);
            *r1 = 10;
            *r2 = 20.0;
            r3.push('0');
        }

        assert_eq!(*c1.borrow(&token), 10);
        assert_eq!(*c2.borrow(&token), 20.0);
        assert_eq!(c3.borrow(&token), "30");

        let (r1, r2) = BrandedCell::borrow_mut_2(&c1, &c2, &mut token);
        *r1 = 100;
        *r2 = 200.0;

        assert_eq!(*c1.borrow(&token), 100);
        assert_eq!(*c2.borrow(&token), 200.0);

        // Reclaim
        heap.free(&mut token, unsafe { c1.into_block() });
        heap.free(&mut token, unsafe { c2.into_block() });
        heap.free(&mut token, unsafe { c3.into_block() });
    });
}

#[test]
#[should_panic(expected = "borrow_mut_2: cells must be distinct")]
fn test_branded_cell_multi_mutable_borrow_panic() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let b = heap.alloc_init(&token, 42).unwrap();
        let c = unsafe { BrandedCell::from_block(b) };

        // This must panic since c and c point to the same block
        let _ = BrandedCell::borrow_mut_2(&c, &c, &mut token);
    });
}

#[test]
fn test_branded_cell_as_ptr_identity() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let b1 = heap.alloc_init(&token, 42).unwrap();
        let b2 = heap.alloc_init(&token, 42).unwrap();

        let c1 = unsafe { BrandedCell::from_block(b1) };
        let c2 = unsafe { BrandedCell::from_block(b2) };
        let c1_copy = c1;

        assert_eq!(c1.as_ptr(), c1_copy.as_ptr());
        assert_ne!(c1.as_ptr(), c2.as_ptr());

        heap.free(&mut token, unsafe { c1.into_block() });
        heap.free(&mut token, unsafe { c2.into_block() });
    });
}

#[test]
fn test_branded_box_from_cell() {
    let counter = std::sync::atomic::AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
        let bbox =
            BrandedBox::new(&heap, &token, DropTracker(&counter)).expect("allocation failed");
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        let cell = bbox.into_cell();
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Reconstruct box from cell
        let bbox_reconstructed = unsafe { BrandedBox::from_cell(&heap, cell) };
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        drop(bbox_reconstructed);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn test_branded_vec_from_boxed_slice_transitions() {
    let counter = std::sync::atomic::AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        // Sized types test
        let mut vec = BrandedVec::new(&heap);
        vec.push(&mut token, DropTracker(&counter)).unwrap();
        vec.push(&mut token, DropTracker(&counter)).unwrap();

        let boxed = vec.into_boxed_slice(&mut token);
        assert_eq!(boxed.len(), 2);
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        let vec_recovered = BrandedVec::from_boxed_slice(boxed);
        assert_eq!(vec_recovered.len(), 2);
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        drop(vec_recovered);
        assert_eq!(counter.load(Ordering::SeqCst), 2);

        // ZST test
        let mut zst_vec = BrandedVec::new(&heap);
        zst_vec.push(&mut token, ()).unwrap();
        zst_vec.push(&mut token, ()).unwrap();

        let zst_boxed = zst_vec.into_boxed_slice(&mut token);
        assert_eq!(zst_boxed.len(), 2);

        let zst_vec_recovered = BrandedVec::from_boxed_slice(zst_boxed);
        assert_eq!(zst_vec_recovered.len(), 2);
        assert_eq!(zst_vec_recovered.capacity(), usize::MAX);
    });
}

#[test]
fn test_branded_vec_into_cell() {
    let counter = std::sync::atomic::AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let mut vec = BrandedVec::new(&heap);
        vec.push(&mut token, DropTracker(&counter)).unwrap();
        vec.push(&mut token, DropTracker(&counter)).unwrap();

        let cell = vec.into_cell(&mut token);
        assert_eq!(cell.borrow(&token).len(), 2);
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        heap.free(&mut token, unsafe { cell.into_block() });
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    });
}

#[test]
fn test_branded_containers_traits_and_vec_ops() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        // --- BrandedBlock ---
        let b1 = heap.alloc_init(&token, 42).unwrap();
        let b2 = heap.alloc_init(&token, 42).unwrap();

        // Pointer
        let _ = format!("{:p}", b1);
        // Debug
        let _ = format!("{:?}", b1);
        // PartialEq/Eq
        assert_eq!(b1, b1);
        assert_ne!(b1, b2);
        // PartialOrd/Ord
        assert!(b1 != b2);
        // Hash
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        use core::hash::Hash;
        b1.hash(&mut hasher);

        // --- BrandedBox ---
        let box1 = BrandedBox::new(&heap, &token, 100).unwrap();
        let box2 = BrandedBox::new(&heap, &token, 200).unwrap();
        // Display/Pointer/Debug
        let _ = format!("{}", box1);
        let _ = format!("{:p}", box1);
        let _ = format!("{:?}", box1);
        // PartialEq/Eq
        assert_eq!(box1, box1);
        assert_ne!(box1, box2);
        // PartialOrd/Ord
        assert_eq!(box1.partial_cmp(&box2), Some(core::cmp::Ordering::Less));
        assert_eq!(box1.cmp(&box2), core::cmp::Ordering::Less);
        // Hash
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        box1.hash(&mut hasher);
        // clone_in
        let box1_clone = box1.clone_in(&token).unwrap();
        assert_eq!(box1, box1_clone);

        // --- BrandedCell ---
        let cell1 = box1.into_cell();
        let cell2 = box2.into_cell();
        let cell1_clone = box1_clone.into_cell();
        // Debug
        let _ = format!("{:?}", cell1);
        // PartialEq/Eq
        assert_eq!(cell1, cell1);
        assert_ne!(cell1, cell2);
        // Hash
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        cell1.hash(&mut hasher);

        // Reclaim BrandedCells
        heap.free(&mut token, unsafe { cell1.into_block() });
        heap.free(&mut token, unsafe { cell2.into_block() });
        heap.free(&mut token, unsafe { cell1_clone.into_block() });

        // --- BrandedVec ---
        let mut vec = BrandedVec::new(&heap);
        vec.push(&mut token, 10).unwrap();
        vec.push(&mut token, 20).unwrap();
        vec.push(&mut token, 30).unwrap();

        // Debug
        let _ = format!("{:?}", vec);
        // PartialEq/Eq
        let vec_clone = vec.clone_in(&mut token).unwrap();
        assert_eq!(vec, vec_clone);
        // PartialOrd/Ord
        assert!(vec <= vec_clone);

        // clear
        let mut vec_clear = vec_clone;
        vec_clear.clear();
        assert_eq!(vec_clear.len(), 0);

        // truncate
        let mut vec_trunc = vec.clone_in(&mut token).unwrap();
        vec_trunc.truncate(1);
        assert_eq!(vec_trunc.len(), 1);
        assert_eq!(vec_trunc[0], 10);

        // reserve & shrink_to_fit
        let mut vec_res = vec.clone_in(&mut token).unwrap();
        vec_res.reserve(&mut token, 100).unwrap();
        assert!(vec_res.capacity() >= 103);
        vec_res.shrink_to_fit(&mut token).unwrap();
        assert_eq!(vec_res.capacity(), 3);

        // insert & remove
        let mut vec_ins = vec.clone_in(&mut token).unwrap();
        vec_ins.insert(&mut token, 1, 99).unwrap();
        assert_eq!(vec_ins.len(), 4);
        assert_eq!(vec_ins[0], 10);
        assert_eq!(vec_ins[1], 99);
        assert_eq!(vec_ins[2], 20);
        assert_eq!(vec_ins[3], 30);

        let removed = vec_ins.remove(1);
        assert_eq!(removed, 99);
        assert_eq!(vec_ins.len(), 3);
        assert_eq!(vec_ins[0], 10);
        assert_eq!(vec_ins[1], 20);
        assert_eq!(vec_ins[2], 30);

        // Clean up
        heap.free(&mut token, b1);
        heap.free(&mut token, b2);
    });
}

#[test]
fn test_branded_vec_in_place_shrink() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        // Minor shrink (within 50% threshold): should shrink in-place
        let mut vec_minor = BrandedVec::with_capacity(&heap, &token, 4).unwrap();
        vec_minor.push(&mut token, 42).unwrap();
        vec_minor.push(&mut token, 43).unwrap();
        vec_minor.push(&mut token, 44).unwrap();

        let orig_ptr_minor = vec_minor.as_slice().as_ptr();
        assert_eq!(vec_minor.capacity(), 4);

        // Shrink minor vector capacity from 4 to 3 (new_size 12 >= 16 / 2)
        vec_minor.shrink_to_fit(&mut token).unwrap();
        assert_eq!(vec_minor.len(), 3);
        assert_eq!(vec_minor.capacity(), 3);
        assert_eq!(vec_minor.as_slice().as_ptr(), orig_ptr_minor);

        // Major shrink (below 50% threshold): should copy & free to release memory
        let mut vec_major = BrandedVec::with_capacity(&heap, &token, 10).unwrap();
        vec_major.push(&mut token, 100).unwrap();
        vec_major.push(&mut token, 101).unwrap();

        let orig_ptr_major = vec_major.as_slice().as_ptr();
        assert_eq!(vec_major.capacity(), 10);

        // Shrink major vector capacity from 10 to 2 (new_size 8 < 40 / 2)
        vec_major.shrink_to_fit(&mut token).unwrap();
        assert_eq!(vec_major.len(), 2);
        assert_eq!(vec_major.capacity(), 2);
        assert_ne!(vec_major.as_slice().as_ptr(), orig_ptr_major);

        // Similar minor shrink check for into_boxed_slice
        let mut vec_slice = BrandedVec::with_capacity(&heap, &token, 4).unwrap();
        vec_slice.push(&mut token, 200).unwrap();
        vec_slice.push(&mut token, 201).unwrap();
        vec_slice.push(&mut token, 202).unwrap();

        let orig_ptr_slice = vec_slice.as_slice().as_ptr();
        let boxed_slice = vec_slice.into_boxed_slice(&mut token);
        assert_eq!(boxed_slice.len(), 3);
        assert_eq!((*boxed_slice).as_ptr(), orig_ptr_slice);
    });
}

#[test]
fn test_branding_thread_bounds() {
    // Structurally, BrandedBox, BrandedVec, and AllocatorToken contain PhantomData<*mut ()>
    // which guarantees that these types are neither Send nor Sync.
    // This ensures they cannot be sent to other threads or accessed concurrently.
    // If they were Send, dropping them on another thread would cause unsynchronized
    // mutation of the thread-local allocator cache.

    // We verify that *mut () itself is !Send + !Sync.
    #[allow(dead_code)]
    trait ImplementsSend {}
    impl<T: Send> ImplementsSend for T {}

    #[allow(dead_code)]
    trait ImplementsSync {}
    impl<T: Sync> ImplementsSync for T {}

    // The following helper type has the same layout and markers as BrandedBox.
    // We can verify that types containing PhantomData<*mut ()> are indeed !Send + !Sync.
    // (Uncommenting the impls below would trigger compile errors, verifying the invariant).
    // struct SendAssert<T: Send>(T);
    // struct SyncAssert<T: Sync>(T);
    // let _ = SendAssert(std::marker::PhantomData::<*mut ()>);
    // let _ = SyncAssert(std::marker::PhantomData::<*mut ()>);
}

#[test]
fn test_branded_vec_extensions() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let mut vec = BrandedVec::new(&heap);
        assert_eq!(vec.len(), 0);

        // Test extend
        vec.extend(&mut token, std::vec![10, 20, 30]).unwrap();
        assert_eq!(vec.len(), 3);
        assert_eq!(vec[0], 10);
        assert_eq!(vec[1], 20);
        assert_eq!(vec[2], 30);

        // Test extend_from_slice
        vec.extend_from_slice(&mut token, &[40, 50]).unwrap();
        assert_eq!(vec.len(), 5);
        assert_eq!(vec[3], 40);
        assert_eq!(vec[4], 50);

        // Test resize (grow)
        vec.resize(&mut token, 7, 99).unwrap();
        assert_eq!(vec.len(), 7);
        assert_eq!(vec[5], 99);
        assert_eq!(vec[6], 99);

        // Test resize (shrink)
        vec.resize(&mut token, 4, 99).unwrap();
        assert_eq!(vec.len(), 4);
        assert_eq!(vec[0], 10);
        assert_eq!(vec[1], 20);
        assert_eq!(vec[2], 30);
        assert_eq!(vec[3], 40);
    });
}
