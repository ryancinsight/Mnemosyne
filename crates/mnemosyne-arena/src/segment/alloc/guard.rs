//! Native page geometry for optional segment guards.

#[cfg(any(test, feature = "segment-header-guards"))]
use mnemosyne_core::constants::PAGE_SIZE;
use mnemosyne_core::constants::SEGMENT_ALIGN;
#[cfg(any(test, feature = "segment-header-guards"))]
use mnemosyne_core::types::Segment;

/// Resolve a guard request that satisfies the system-page preconditions.
/// Unknown or unsupported geometry leaves this best-effort feature inactive.
pub(crate) fn native_guard_size(minimum: usize) -> Option<usize> {
    guard_size_for_page(minimum, system_page_size()?)
}

/// Reject a header request that would cover any live metadata in Page 0.
#[cfg(any(test, feature = "segment-header-guards"))]
pub(crate) fn header_guard_offset(size: usize) -> Option<usize> {
    PAGE_SIZE
        .checked_sub(size)
        .filter(|offset| size != 0 && *offset >= core::mem::size_of::<Segment>())
}

fn guard_size_for_page(minimum: usize, page: usize) -> Option<usize> {
    if !minimum.is_power_of_two()
        || !page.is_power_of_two()
        || minimum > SEGMENT_ALIGN
        || page > SEGMENT_ALIGN
    {
        return None;
    }
    // Both are powers of two, so their maximum is a multiple of each.
    Some(minimum.max(page))
}

#[cfg(unix)]
fn system_page_size() -> Option<usize> {
    // SAFETY: _SC_PAGESIZE is a valid sysconf selector and has no pointer args.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(page).ok()
}

#[cfg(windows)]
fn system_page_size() -> Option<usize> {
    use core::mem::MaybeUninit;
    use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};

    let mut info = MaybeUninit::<SYSTEM_INFO>::uninit();
    // SAFETY: GetSystemInfo initializes the entire valid, writable SYSTEM_INFO.
    let info = unsafe {
        GetSystemInfo(info.as_mut_ptr());
        info.assume_init()
    };
    usize::try_from(info.dwPageSize).ok()
}

#[cfg(not(any(unix, windows)))]
fn system_page_size() -> Option<usize> {
    None
}

#[cfg(test)]
mod tests {
    use super::{PAGE_SIZE, SEGMENT_ALIGN, guard_size_for_page, header_guard_offset};

    #[test]
    fn requests_cover_the_minimum_and_whole_system_pages() {
        for minimum in [4096, 16384] {
            for page in [4096, 16384, 65536] {
                let size = guard_size_for_page(minimum, page).expect("supported page geometry");
                assert!(size >= minimum);
                assert!(size.is_multiple_of(page));
                assert_eq!(size, minimum.max(page));
            }
        }
    }

    #[test]
    fn unsupported_geometry_never_produces_a_guard_request() {
        for page in [0, 3, SEGMENT_ALIGN * 2] {
            assert_eq!(guard_size_for_page(4096, page), None);
        }
        assert_eq!(guard_size_for_page(0, 4096), None);
    }

    #[test]
    fn header_guards_leave_metadata_outside_the_protected_range() {
        assert_eq!(header_guard_offset(4096), Some(PAGE_SIZE - 4096));
        assert_eq!(header_guard_offset(16384), Some(PAGE_SIZE - 16384));
        assert_eq!(header_guard_offset(65536), None);
        assert_eq!(header_guard_offset(0), None);
    }
}
