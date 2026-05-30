//! NUMA querying utilities with thread-local caching.

extern crate std;

std::thread_local! {
    static CACHED_NODE: core::cell::Cell<Option<u32>> = const { core::cell::Cell::new(None) };
}

/// Returns the NUMA node of the calling thread from thread-local cache.
#[inline]
pub fn current_numa_node() -> u32 {
    CACHED_NODE.with(|cell| {
        if let Some(node) = cell.get() {
            node
        } else {
            let node = query_numa_node_os();
            cell.set(Some(node));
            node
        }
    })
}

/// Forces a refresh of the cached NUMA node from the OS and returns it.
#[inline]
pub fn refresh_numa_node() -> u32 {
    let node = query_numa_node_os();
    CACHED_NODE.with(|cell| cell.set(Some(node)));
    node
}

#[inline(never)]
fn query_numa_node_os() -> u32 {
    #[cfg(target_os = "linux")]
    {
        let mut cpu = 0u32;
        let mut node = 0u32;
        unsafe {
            let mut ret: isize;
            core::arch::asm!(
                "syscall",
                in("rax") 309isize, // __NR_getcpu
                in("rdi") &mut cpu as *mut u32,
                in("rsi") &mut node as *mut u32,
                in("rdx") core::ptr::null_mut::<u8>(),
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack, preserves_flags)
            );
            if ret == 0 {
                node
            } else {
                0
            }
        }
    }
    #[cfg(windows)]
    {
        unsafe {
            extern "system" {
                fn GetCurrentProcessorNumber() -> u32;
                fn GetNumaProcessorNode(Processor: u8, NodeNumber: *mut u8) -> i32;
            }
            let cpu = GetCurrentProcessorNumber();
            let mut node = 0u8;
            if GetNumaProcessorNode(cpu as u8, &mut node) != 0 {
                node as u32
            } else {
                0
            }
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        0
    }
}
