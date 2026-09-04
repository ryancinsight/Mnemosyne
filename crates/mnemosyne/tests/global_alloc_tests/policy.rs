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

    // SAFETY: Under standard execution, accessing freed memory is undefined behavior.
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
fn test_cuda_backends() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    #[cfg(windows)]
    {
        // Skip on Windows: the WDDM driver does not support concurrent CPU access
        // to managed memory from parallel test processes executed by nextest.
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

        assert_cuda_backend_round_trip::<CudaUnifiedBackend>(42, "unified");
        assert_cuda_backend_round_trip::<CudaDeviceBackend>(43, "device");
        assert_cuda_backend_round_trip::<CudaHbmBackend>(44, "HBM");
        assert_cuda_backend_round_trip::<CudaGddrBackend>(45, "GDDR");
        assert_cuda_backend_round_trip::<CudaHostPinnedBackend>(46, "host pinned");

        // Verify is_cuda_available is callable
        let _ = is_cuda_available();

        unsafe { mnemosyne_backend::backends::cuda::destroy_temp_context(ctx) };
    }
}

#[cfg(not(windows))]
fn assert_cuda_backend_round_trip<B>(value: u8, name: &str)
where
    B: mnemosyne_arena::HasSegmentPool + mnemosyne::LocalAllocatorSelector<B>,
{
    let allocator = MnemosyneAllocator::<StandardPolicy, B>::new();
    let layout = Layout::from_size_align(128, 8).expect("128-byte 8-aligned Layout is valid");
    let ptr = unsafe { allocator.alloc(layout) };
    assert!(!ptr.is_null(), "CUDA {name} backend allocation failed");

    unsafe {
        ptr.write(value);
        assert_eq!(ptr.read(), value);
        allocator.dealloc(ptr, layout);
    }

    let stats = memory_stats_generic::<StandardPolicy, B>();
    assert_eq!(
        stats.current_thread_live_allocations, 0,
        "CUDA {name} backend retained a live allocation"
    );
}
#[test]
fn test_hardened_policy_features() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");

    use mnemosyne::{AllocPolicy, HardenedPolicy, mitigations};

    // Verify policy constants are as expected.
    assert!(HardenedPolicy::ENABLE_POISONING);
    assert!(HardenedPolicy::ZERO_INITIALIZE);
    assert!(HardenedPolicy::ENABLE_FREE_LIST_ENCRYPTION);
    assert!(HardenedPolicy::RANDOMIZE_ALLOCATION);
    assert!(HardenedPolicy::DELAY_PAGE_WAKE);

    // MITIGATION_FLAGS includes all implemented bits.
    assert_ne!(HardenedPolicy::MITIGATION_FLAGS, mitigations::NONE);
    assert_ne!(HardenedPolicy::POLICY_FINGERPRINT, 0);

    // Allocate with HardenedPolicy and verify zero-initialization.
    let allocator = MnemosyneAllocator::<HardenedPolicy>::new();
    let layout = Layout::from_size_align(64, 8).expect("64-byte 8-aligned Layout is valid");

    let ptr = unsafe { allocator.alloc(layout) };
    assert!(!ptr.is_null(), "hardened-policy allocation failed");

    // Verify zero-initialization.
    let slice = unsafe { core::slice::from_raw_parts(ptr, 64) };
    for &byte in slice {
        assert_eq!(
            byte, 0,
            "HardenedPolicy allocation must be zero-initialized"
        );
    }

    // Write a pattern and free; the free path writes a canary.
    unsafe { core::ptr::write_bytes(ptr, 0xBB, 64) };
    unsafe { allocator.dealloc(ptr, layout) };

    // Allocate again -- the new block comes from the same pool but is
    // zero-initialized by HardenedPolicy.
    let ptr2 = unsafe { allocator.alloc(layout) };
    assert!(!ptr2.is_null());
    let slice2 = unsafe { core::slice::from_raw_parts(ptr2, 64) };
    for &byte in slice2 {
        assert_eq!(
            byte, 0,
            "second HardenedPolicy allocation must be zero-initialized"
        );
    }
    unsafe { allocator.dealloc(ptr2, layout) };
}

#[test]
fn test_policy_fingerprint_uniqueness() {
    use mnemosyne::{AllocPolicy, HardenedPolicy, SecurePolicy, StandardPolicy};

    // All three production policies must have distinct fingerprints.
    let std_fp = StandardPolicy::POLICY_FINGERPRINT;
    let sec_fp = SecurePolicy::POLICY_FINGERPRINT;
    let hrd_fp = HardenedPolicy::POLICY_FINGERPRINT;

    assert_ne!(
        std_fp, sec_fp,
        "Standard and Secure must have distinct fingerprints"
    );
    assert_ne!(
        std_fp, hrd_fp,
        "Standard and Hardened must have distinct fingerprints"
    );
    assert_ne!(
        sec_fp, hrd_fp,
        "Secure and Hardened must have distinct fingerprints"
    );

    // StandardPolicy fingerprint: all bool flags false -> low bits zero.
    assert_eq!(std_fp & 0x1F, 0, "StandardPolicy has no bool flags set");
}
