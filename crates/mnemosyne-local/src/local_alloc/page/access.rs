use core::ptr::NonNull;
use mnemosyne_core::types::Page;

/// Reconstitutes a cached page address for the current metadata access.
///
/// Page-list links outlive individual allocation calls. Because page metadata
/// and user blocks share one mapping, retaining reference-derived provenance
/// across those calls is invalid under Rust's aliasing model. The intrusive
/// lists therefore carry addresses and refresh exposed provenance at each use.
#[inline(always)]
pub(crate) fn refresh_page_pointer(page: NonNull<Page>) -> NonNull<Page> {
    // SAFETY: a `NonNull<Page>` list entry denotes a live page by every caller
    // contract; reconstructing the same non-zero address preserves that fact.
    unsafe {
        NonNull::new_unchecked(core::ptr::with_exposed_provenance_mut(
            page.as_ptr().expose_provenance(),
        ))
    }
}

/// Raw-pointer form of [`refresh_page_pointer`].
///
/// # Safety
///
/// `page` must be non-null and identify a live page.
#[inline(always)]
pub(crate) unsafe fn refresh_raw_page_pointer(page: *mut Page) -> *mut Page {
    core::ptr::with_exposed_provenance_mut(page.expose_provenance())
}
