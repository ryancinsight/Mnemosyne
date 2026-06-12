use super::*;

#[inline(never)]
fn do_integration_alloc() -> *mut u8 {
    let layout = Layout::from_size_align(64, 8).expect("valid layout");
    unsafe { ALLOCATOR.alloc(layout) }
}

struct ProfilerIntegrationGuard;

impl ProfilerIntegrationGuard {
    fn new() -> Self {
        mnemosyne_prof::reset_profiler_for_testing();
        Self
    }
}

impl Drop for ProfilerIntegrationGuard {
    fn drop(&mut self) {
        disable_leak_detector();
        mnemosyne_prof::reset_profiler_for_testing();
    }
}

struct GlobalAllocationGuard {
    ptr: *mut u8,
    layout: Layout,
}

impl GlobalAllocationGuard {
    fn new(ptr: *mut u8, layout: Layout) -> Self {
        assert!(
            !ptr.is_null(),
            "global allocator returned null for guarded allocation"
        );
        Self { ptr, layout }
    }
}

impl Drop for GlobalAllocationGuard {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { ALLOCATOR.dealloc(self.ptr, self.layout) };
            self.ptr = core::ptr::null_mut();
        }
    }
}

#[test]
fn test_leak_detector_integration() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");

    thread::spawn(|| {
        let _profiler_guard = ProfilerIntegrationGuard::new();
        enable_leak_detector();
        assert!(is_leak_detector_enabled());

        let ptr = do_integration_alloc();
        let layout = Layout::from_size_align(64, 8).expect("valid layout");
        let _allocation_guard = GlobalAllocationGuard::new(ptr, layout);

        disable_leak_detector();
        assert!(!is_leak_detector_enabled());

        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("mnemosyne_integration_leaks.txt");
        let path_str = path.to_str().expect("valid temp path");

        let dump_res = dump_leaks(path_str);
        assert!(dump_res.is_ok(), "dump_leaks failed: {:?}", dump_res.err());
        let count = dump_res.expect("dump_leaks failed after positive status check");
        assert!(
            count >= 1,
            "Expected at least 1 leak captured (got {})",
            count
        );

        // Verify the file was created and contains the backtrace info.
        let content = std::fs::read_to_string(&path).expect("failed to read leak report");
        assert!(
            content.contains("do_integration_alloc"),
            "Stack trace missing integration test function symbol: {}",
            content
        );

        let _ = std::fs::remove_file(&path);
    })
    .join()
    .expect("leak detector integration thread panicked");
    mnemosyne_prof::reset_profiler_for_testing();
}
