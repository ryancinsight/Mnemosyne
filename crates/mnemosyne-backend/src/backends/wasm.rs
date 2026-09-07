//! WebAssembly page allocation backend using the host global allocator.

use core::alloc::Layout;

use mnemosyne_core::MemoryBackend;

/// WebAssembly backend for page-aligned allocations.
///
/// WebAssembly does not expose OS mapping, protection, reset, or decommit
/// primitives through the portable target ABI. Allocations therefore use the
/// process global allocator with the allocator's page alignment contract, and
/// unsupported page operations report `false` through the trait defaults.
pub struct WasmBackend;

impl MemoryBackend for WasmBackend {
    /// WebAssembly has no portable page-reset primitive.
    const SUPPORTS_PAGE_RESET: bool = false;
    /// WebAssembly has no portable memory-protection primitive.
    const SUPPORTS_MAKE_GUARD: bool = false;
    /// WebAssembly has no portable decommit primitive.
    const SUPPORTS_DECOMMIT: bool = false;

    /// Allocates page-aligned memory from the WebAssembly global allocator.
    ///
    /// # Safety
    ///
    /// `size` must be non-zero and page-aligned, as required by
    /// [`MemoryBackend::allocate`].
    unsafe fn allocate(size: usize) -> *mut u8 {
        let Ok(layout) = Layout::from_size_align(size, mnemosyne_core::PAGE_ALIGN) else {
            return core::ptr::null_mut();
        };
        // SAFETY: the caller supplies the non-zero, page-aligned size required
        // by the trait contract; `layout` carries the matching alignment.
        unsafe { alloc::alloc::alloc(layout) }
    }

    /// Releases memory previously returned by [`MemoryBackend::allocate`].
    ///
    /// # Safety
    ///
    /// `ptr` must be a live allocation returned by this backend, and `size`
    /// must be the original page-aligned allocation size.
    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        if ptr.is_null() {
            return false;
        }
        let Ok(layout) = Layout::from_size_align(size, mnemosyne_core::PAGE_ALIGN) else {
            return false;
        };
        // SAFETY: the caller supplies the original pointer and size, so this
        // layout is identical to the one used by `allocate`.
        unsafe { alloc::alloc::dealloc(ptr, layout) };
        true
    }
}
