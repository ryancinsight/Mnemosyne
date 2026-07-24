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
