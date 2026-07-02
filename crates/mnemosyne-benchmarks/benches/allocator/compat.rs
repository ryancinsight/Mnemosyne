/// Jemalloc comparator abstraction.
///
/// Exposes a single `Jemalloc` `GlobalAlloc` and a `usable_size` helper so the
/// benchmark bodies are identical across platforms. On non-Windows targets this
/// is `tikv-jemallocator`; on Windows (under the `system-jemalloc` feature) it
/// is a thin `GlobalAlloc` over a system-installed `libjemalloc_s.a`, using
/// jemalloc's sized `je_*x` API for parity with `tikv-jemallocator`. The module
/// only exists when `build.rs` determined jemalloc is available
/// (`jemalloc_available` cfg); every use site is gated the same way.
#[cfg(jemalloc_available)]
pub mod bench_jemalloc {
    #[cfg(not(windows))]
    pub use tikv_jemallocator::{Jemalloc, usable_size};

    #[cfg(windows)]
    pub use sys::{SystemJemalloc as Jemalloc, usable_size};

    #[cfg(windows)]
    mod sys {
        use core::alloc::{GlobalAlloc, Layout};

        // Link the system jemalloc static library directly from this object.
        // `build.rs` emits the `-L` search path (e.g. MSYS2 `ucrt64/lib`); the
        // `#[link]` attribute embeds the `-l` requirement in the bench crate
        // itself, which is more reliable than build-script `rustc-link-lib`
        // propagation across the separate bench crate.
        #[link(name = "jemalloc_s", kind = "static")]
        unsafe extern "C" {
            fn je_mallocx(size: usize, flags: i32) -> *mut u8;
            fn je_rallocx(ptr: *mut u8, size: usize, flags: i32) -> *mut u8;
            fn je_sdallocx(ptr: *mut u8, size: usize, flags: i32);
            fn je_malloc_usable_size(ptr: *const u8) -> usize;
        }

        /// jemalloc `MALLOCX_ZERO` flag (request zeroed memory).
        const MALLOCX_ZERO: i32 = 0x40;

        /// Encodes the jemalloc `mallocx`/`sdallocx`/`rallocx` flags for a
        /// layout. Mirrors `tikv-jemallocator`'s `layout_to_flags`: no
        /// alignment flag when the alignment is within jemalloc's natural
        /// size-class guarantee (`<= 16` on 64-bit and `<= size`); otherwise
        /// `MALLOCX_ALIGN(align)`, which is `log2(align)` for power-of-two
        /// alignments.
        #[inline]
        fn flags(layout: Layout) -> i32 {
            let align = layout.align();
            if align <= 16 && align <= layout.size() {
                0
            } else {
                align.trailing_zeros() as i32
            }
        }

        /// `GlobalAlloc` over the system jemalloc static library.
        pub struct SystemJemalloc;

        // Safety: jemalloc's allocator is thread-safe and satisfies the
        // `GlobalAlloc` contract; the sized `je_*x` calls forward layout size
        // and alignment exactly.
        unsafe impl GlobalAlloc for SystemJemalloc {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                unsafe { je_mallocx(layout.size(), flags(layout)) }
            }

            unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
                unsafe { je_mallocx(layout.size(), flags(layout) | MALLOCX_ZERO) }
            }

            unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
                unsafe {
                    je_sdallocx(ptr, layout.size(), flags(layout));
                }
            }

            unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
                unsafe {
                    // Safety: new_size is nonzero and align is the original
                    // power-of-two alignment, a valid layout.
                    let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
                    je_rallocx(ptr, new_size, flags(new_layout))
                }
            }
        }

        /// Mirrors `tikv_jemallocator::usable_size`.
        ///
        /// # Safety
        ///
        /// `ptr` must have been returned by this allocator and still be live.
        pub unsafe fn usable_size<T>(ptr: *const T) -> usize {
            unsafe { je_malloc_usable_size(ptr as *const u8) }
        }
    }
}

// MinGW linker compatibility stubs for snmalloc
#[unsafe(no_mangle)]
pub static mut __imp_VirtualAlloc2FromApp: unsafe extern "system" fn(
    *mut core::ffi::c_void,
    *mut core::ffi::c_void,
    usize,
    u32,
    u32,
    *mut core::ffi::c_void,
    u32,
) -> *mut core::ffi::c_void = fallback_virtual_alloc_2_from_app;

unsafe extern "system" fn fallback_virtual_alloc_2_from_app(
    h_process: *mut core::ffi::c_void,
    base_address: *mut core::ffi::c_void,
    size: usize,
    allocation_type: u32,
    protect: u32,
    extended_parameters: *mut core::ffi::c_void,
    parameter_count: u32,
) -> *mut core::ffi::c_void {
    type FuncType = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        usize,
        u32,
        u32,
        *mut core::ffi::c_void,
        u32,
    ) -> *mut core::ffi::c_void;

    static REAL_FUNC: std::sync::OnceLock<Option<FuncType>> = std::sync::OnceLock::new();

    let func_opt = REAL_FUNC.get_or_init(|| {
        unsafe extern "system" {
            fn GetModuleHandleA(lpModuleName: *const u8) -> *mut core::ffi::c_void;
            fn GetProcAddress(
                hModule: *mut core::ffi::c_void,
                lpProcName: *const u8,
            ) -> *mut core::ffi::c_void;
        }

        // Safety: the imported Win32 functions are called with static nul-terminated
        // symbol names and the returned handles are checked for null before use.
        unsafe {
            let kernel32 = GetModuleHandleA(c"kernel32.dll".as_ptr() as *const u8);
            if !kernel32.is_null() {
                let func_ptr =
                    GetProcAddress(kernel32, c"VirtualAlloc2FromApp".as_ptr() as *const u8);
                if !func_ptr.is_null() {
                    // Safety: `func_ptr` was resolved for `VirtualAlloc2FromApp`,
                    // whose ABI matches `FuncType`.
                    return Some(core::mem::transmute::<*mut core::ffi::c_void, FuncType>(
                        func_ptr,
                    ));
                }
                let func_ptr2 = GetProcAddress(kernel32, c"VirtualAlloc2".as_ptr() as *const u8);
                if !func_ptr2.is_null() {
                    // Safety: `func_ptr2` was resolved for `VirtualAlloc2`,
                    // whose ABI matches `FuncType`.
                    return Some(core::mem::transmute::<*mut core::ffi::c_void, FuncType>(
                        func_ptr2,
                    ));
                }
            }
            None
        }
    });

    if let Some(func) = func_opt {
        // Safety: `func` is a checked dynamic symbol matching `FuncType`.
        unsafe {
            func(
                h_process,
                base_address,
                size,
                allocation_type,
                protect,
                extended_parameters,
                parameter_count,
            )
        }
    } else {
        unsafe extern "system" {
            fn VirtualAlloc(
                lpAddress: *mut core::ffi::c_void,
                dwSize: usize,
                flAllocationType: u32,
                flProtect: u32,
            ) -> *mut core::ffi::c_void;
        }
        // Safety: forwards the raw allocation request to the OS fallback with the
        // same parameters supplied to the missing `VirtualAlloc2*` entry point.
        unsafe { VirtualAlloc(base_address, size, allocation_type, protect) }
    }
}
