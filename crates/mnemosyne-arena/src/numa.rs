//! NUMA querying utilities backed by Themis placement law.

/// Returns the NUMA node of the calling thread from thread-local cache.
#[inline]
pub fn current_numa_node() -> u32 {
    themis::current_numa_node().get()
}

/// Forces a refresh of the cached NUMA node from the OS and returns it.
#[inline]
pub fn refresh_numa_node() -> u32 {
    themis::refresh_current_numa_node().get()
}

/// Binds a freshly `mmap`-allocated segment to the given NUMA node via
/// `mbind(MPOL_BIND)`.
///
/// On Linux ≥ 2.6.7, `mbind` enforces first-touch NUMA locality without
/// relying on the process's default memory policy. It is a best-effort hint:
/// failures (unsupported kernel, no `CAP_SYS_NICE` in restricted environments,
/// non-NUMA machines) are silently ignored — the mapping stays valid and the
/// kernel falls back to its own placement.
///
/// Call this immediately after a successful `B::allocate` for a new segment,
/// before the segment is published to any thread, so the call races nothing.
///
/// # Safety
///
/// `ptr` must point to an OS mapping of at least `len` bytes and `numa_node`
/// must be a valid node reported by the kernel.  The page range `[ptr, ptr +
/// len)` must not be concurrently accessed while `mbind` is executing (it is
/// safe to call before the segment is published).
#[cfg(all(target_os = "linux", not(miri)))]
#[inline]
pub unsafe fn bind_segment_to_numa_node(ptr: *mut u8, len: usize, numa_node: u32) {
    use core::ffi::c_int;
    use core::ffi::c_ulong;
    use core::ffi::c_void;

    // `MPOL_BIND = 2` — allocate only on nodes in the nodemask.
    const MPOL_BIND: c_int = 2;

    unsafe extern "C" {
        fn mbind(
            addr: *mut c_void,
            len: c_ulong,
            mode: c_int,
            nodemask: *const c_ulong,
            maxnode: c_ulong,
            flags: c_int,
        ) -> c_int;
    }

    let nodemask: c_ulong = (1u64 as c_ulong).wrapping_shl(numa_node);
    // `maxnode = 64` covers the 64-bit nodemask above.
    // SAFETY: `ptr` addresses a live OS mapping of `len` bytes (caller
    // contract); `nodemask` is a stack variable whose address is valid for
    // the duration of the syscall; no concurrent access to the range.
    let _ = unsafe {
        mbind(
            ptr as *mut c_void,
            len as c_ulong,
            MPOL_BIND,
            &nodemask,
            64,
            0,
        )
    };
}

/// No-op on non-Linux or Miri: the caller can call this unconditionally.
///
/// # Safety
///
/// This body does nothing, but the contract matches the Linux definition so a
/// caller compiles against one signature on every target: `ptr` must point to
/// an OS mapping of at least `len` bytes, `numa_node` must be a valid node
/// reported by the kernel, and the range `[ptr, ptr + len)` must not be
/// concurrently accessed during the call. Holding to it here keeps a caller
/// portable to a target where the call is not a no-op.
#[cfg(not(all(target_os = "linux", not(miri))))]
#[inline]
pub unsafe fn bind_segment_to_numa_node(_ptr: *mut u8, _len: usize, _numa_node: u32) {}
