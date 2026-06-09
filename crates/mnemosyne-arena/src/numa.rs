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
