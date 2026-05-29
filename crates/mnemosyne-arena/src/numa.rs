//! NUMA querying utilities.

/// Returns the NUMA node of the calling thread.
#[inline]
pub fn current_numa_node() -> u32 {
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
