use crate::types::{Page, Segment};
use ::std::alloc::{Layout, alloc_zeroed, dealloc};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RandomizedTestPolicy;

impl crate::policy::private::Sealed for RandomizedTestPolicy {}
impl crate::policy::AllocPolicy for RandomizedTestPolicy {
    const ENABLE_POISONING: bool = false;
    const ZERO_INITIALIZE: bool = false;
    const RANDOMIZE_ALLOCATION: bool = true;
}

fn segment_layout() -> Layout {
    Layout::from_size_align(
        crate::constants::SEGMENT_SIZE,
        crate::constants::SEGMENT_SIZE,
    )
    .expect("segment layout uses equal power-of-two size and alignment")
}

#[test]
fn page_struct_size_stays_within_one_cache_line() {
    // Page metadata is hot: every allocation reads and writes
    // `page.free`, `page.alloc_count`, and `page.block_size`. Keeping
    // the struct within a single 64-byte cache line on 64-bit targets
    // ensures the fast path touches only one cache line per page
    // operation.
    assert!(
        core::mem::size_of::<Page>() <= 64,
        "Page exceeds one 64-byte cache line ({} bytes)",
        core::mem::size_of::<Page>()
    );
}

#[test]
fn segment_default_free_list_mode_matches_standard_runtime_state() {
    let segment = Segment::default();
    assert!(!segment.free_list_encrypted);
    assert!(unsafe { Segment::free_list_mode_matches(&segment, false) });
    assert!(!unsafe { Segment::free_list_mode_matches(&segment, true) });
}

#[test]
fn free_list_mode_matches_segment_state() {
    let layout = segment_layout();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(
        !segment_ptr.is_null(),
        "alloc_zeroed failed to allocate segment"
    );
    unsafe { Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0) };

    assert!(unsafe { Segment::free_list_mode_matches(segment_ptr, false) });
    assert!(!unsafe { Segment::free_list_mode_matches(segment_ptr, true) });

    unsafe { (*segment_ptr).free_list_encrypted = true };
    assert!(unsafe { Segment::free_list_mode_matches(segment_ptr, true) });
    assert!(!unsafe { Segment::free_list_mode_matches(segment_ptr, false) });

    unsafe {
        dealloc(segment_ptr as *mut u8, layout);
    }
}

#[test]
// Re-executes the test binary to observe the abort; Miri cannot spawn a
// subprocess, so the assertion is unobservable under it rather than failing.
#[cfg_attr(miri, ignore = "spawns a subprocess")]
fn reclaim_thread_free_if_present_rejects_mismatched_mode() {
    if std::env::var_os("MNEMOSYNE_RECLAIM_MODE_GUARD").is_some() {
        let layout = segment_layout();
        let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
        assert!(
            !segment_ptr.is_null(),
            "alloc_zeroed failed to allocate segment"
        );
        unsafe {
            Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0);
            (*segment_ptr).free_list_encrypted = true;
            Page::reclaim_thread_free_if_present_in_segment(segment_ptr, 1, false);
        }
        panic!("raw reclaim path should have aborted on free-list mode mismatch");
    }

    let output = std::process::Command::new(
        std::env::current_exe().expect("invariant: a test binary knows its own path"),
    )
    .env("MNEMOSYNE_RECLAIM_MODE_GUARD", "1")
    .arg("reclaim_thread_free_if_present_rejects_mismatched_mode")
    .arg("--nocapture")
    .output()
    .expect("child test process should run");

    assert!(
        !output.status.success(),
        "raw reclaim path must abort on a mode mismatch; child stderr: {}",
        std::string::String::from_utf8_lossy(&output.stderr)
    );
}

// -- Backward-edge free canary tests ------------------------------------------

mod canary;
mod free_lists;
mod huge_mappings;
mod queue;
