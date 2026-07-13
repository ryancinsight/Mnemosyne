use super::*;

#[test]
fn test_large_alignment() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let alignments = [32 * 1024, 64 * 1024, 128 * 1024, 2 * 1024 * 1024];
    for align in alignments {
        let layout = Layout::from_size_align(4096, align)
            .expect("large-alignment test table contains valid Layout alignments");
        let ptr = unsafe { ALLOCATOR.alloc(layout) };
        assert!(!ptr.is_null(), "Allocation failed for alignment {}", align);
        assert_eq!(
            ptr as usize % align,
            0,
            "Pointer {:?} is not aligned to {}",
            ptr,
            align
        );
        // Verify writing and reading to make sure alignment bounds check out.
        unsafe {
            ptr.write(0xAA);
            assert_eq!(ptr.read(), 0xAA);
            ptr.add(4095).write(0x55);
            assert_eq!(ptr.add(4095).read(), 0x55);
        }
        unsafe { ALLOCATOR.dealloc(ptr, layout) };
    }
}

#[test]
fn test_secure_policy() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let allocator = MnemosyneAllocator::<SecurePolicy>::new();
    let layout = Layout::from_size_align(128, 8).expect("128-byte 8-aligned Layout is valid");

    // 1. Test zero-initialization
    let ptr = unsafe { allocator.alloc(layout) };
    assert!(!ptr.is_null(), "secure-policy allocation failed");

    // Verify that the memory is indeed zero-initialized
    let slice = unsafe { core::slice::from_raw_parts(ptr, 128) };
    for &byte in slice {
        assert_eq!(byte, 0, "Byte was not zero-initialized");
    }

    // 2. Test memory poisoning on deallocation.
    // We write some sentinel values before freeing to ensure it's overwritten by poison bytes.
    unsafe {
        core::ptr::write_bytes(ptr, 0x41, 128);
    }

    unsafe { allocator.dealloc(ptr, layout) };

    // Safety: Under standard execution, accessing freed memory is undefined behavior.
    // However, in this controlled integration test, we verify that the poisoning logic
    // has overwritten the memory. The segment cache retains pages so the memory
    // remains mapped and readable for testing.
    let skip_bytes =
        core::mem::size_of::<Option<core::ptr::NonNull<mnemosyne_core::types::Block>>>();
    for i in skip_bytes..128 {
        let val = unsafe { ptr.add(i).read() };
        assert_eq!(
            val, 0xDE,
            "Byte at index {} was not poisoned (got 0x{:02X}, expected 0xDE)",
            i, val
        );
    }
}

#[test]
fn test_cuda_unified_backend() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    #[cfg(windows)]
    {
        // Skip on Windows: the WDDM driver does not support concurrent CPU access
        // to managed memory from parallel test processes executed by nextest.
    }
    // Test WgpuStagingBackend
    {
        use mnemosyne_backend::{WgpuCallbacks, register_wgpu_callbacks};

        unsafe extern "C" fn mock_alloc(size: usize) -> *mut u8 {
            unsafe { std::alloc::alloc(Layout::from_size_align_unchecked(size, 8)) }
        }

        unsafe extern "C" fn mock_dealloc(ptr: *mut u8, size: usize) -> bool {
            unsafe {
                std::alloc::dealloc(ptr, Layout::from_size_align_unchecked(size, 8));
                true
            }
        }

        // SAFETY: the test callbacks share one System layout contract and do
        // not unwind across their FFI boundary.
        static CALLBACKS: WgpuCallbacks = unsafe { WgpuCallbacks::new(mock_alloc, mock_dealloc) };
        register_wgpu_callbacks(&CALLBACKS).expect("test callback pair must register");

        let allocator = MnemosyneAllocator::<StandardPolicy, WgpuStagingBackend>::new();
        let layout = Layout::from_size_align(128, 8).expect("128-byte 8-aligned Layout is valid");
        let ptr = unsafe { allocator.alloc(layout) };
        assert!(!ptr.is_null(), "Wgpu staging backend allocation failed");

        unsafe {
            ptr.write(99);
            assert_eq!(ptr.read(), 99);
            allocator.dealloc(ptr, layout);
        }
    }

    #[cfg(not(windows))]
    {
        if !is_cuda_available() {
            return;
        }
        let ctx = unsafe { mnemosyne_backend::backends::cuda::create_temp_context() };
        if ctx.is_null() {
            return;
        }

        // Test CudaUnifiedBackend
        {
            let allocator = MnemosyneAllocator::<StandardPolicy, CudaUnifiedBackend>::new();
            let layout =
                Layout::from_size_align(128, 8).expect("128-byte 8-aligned Layout is valid");
            let ptr = unsafe { allocator.alloc(layout) };
            assert!(!ptr.is_null(), "CUDA unified backend allocation failed");

            unsafe {
                ptr.write(42);
                assert_eq!(ptr.read(), 42);
                allocator.dealloc(ptr, layout);
            }

            let stats = memory_stats_generic::<CudaUnifiedBackend>();
            assert_eq!(stats.current_thread_live_allocations, 0);
        }

        // Test CudaDeviceBackend
        {
            let allocator = MnemosyneAllocator::<StandardPolicy, CudaDeviceBackend>::new();
            let layout =
                Layout::from_size_align(128, 8).expect("128-byte 8-aligned Layout is valid");
            let ptr = unsafe { allocator.alloc(layout) };
            assert!(!ptr.is_null(), "CUDA device backend allocation failed");

            unsafe {
                ptr.write(43);
                assert_eq!(ptr.read(), 43);
                allocator.dealloc(ptr, layout);
            }

            let stats = memory_stats_generic::<CudaDeviceBackend>();
            assert_eq!(stats.current_thread_live_allocations, 0);
        }

        // Test CudaHostPinnedBackend
        {
            let allocator = MnemosyneAllocator::<StandardPolicy, CudaHostPinnedBackend>::new();
            let layout =
                Layout::from_size_align(128, 8).expect("128-byte 8-aligned Layout is valid");
            let ptr = unsafe { allocator.alloc(layout) };
            assert!(!ptr.is_null(), "CUDA host pinned backend allocation failed");

            unsafe {
                ptr.write(44);
                assert_eq!(ptr.read(), 44);
                allocator.dealloc(ptr, layout);
            }

            let stats = memory_stats_generic::<CudaHostPinnedBackend>();
            assert_eq!(stats.current_thread_live_allocations, 0);
        }

        // Verify is_cuda_available is callable
        let _ = is_cuda_available();

        unsafe { mnemosyne_backend::backends::cuda::destroy_temp_context(ctx) };
    }
}
