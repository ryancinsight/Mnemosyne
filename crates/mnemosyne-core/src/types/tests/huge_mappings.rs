//! The suffix a huge allocation reports, at an ordinary base and at the
//! top of the address space.

use crate::types::Segment;

#[test]
fn huge_mapping_suffix_uses_raw_mapping_base() {
    let mut segment_storage = core::mem::MaybeUninit::<Segment>::uninit();
    let segment = segment_storage.as_mut_ptr();
    let mut mapping = std::vec![0_u8; 0x4000];
    let raw = mapping.as_mut_ptr();
    unsafe {
        Segment::initialize(segment, raw, 0);
        (*segment).pages[0].block_size = 0x4000;
    }

    let expected_key =
        (segment as usize).wrapping_add(crate::constants::PAGE_SIZE) ^ (usize::MAX / 3);
    assert_eq!(
        unsafe { (*segment).keys[1] },
        expected_key,
        "segment page keys must use the pointer-width mask"
    );

    let user_ptr = unsafe { raw.add(0x1800) }.cast_const();
    let suffix = unsafe { (*segment).huge_mapping_suffix_from(user_ptr) };

    assert_eq!(
        suffix, 0x2800,
        "huge usable suffix must be raw_alloc_ptr + block_size - user_ptr"
    );
}
#[test]
fn huge_mapping_suffix_is_address_space_safe() {
    let mut segment_storage = core::mem::MaybeUninit::<Segment>::uninit();
    let segment = segment_storage.as_mut_ptr();
    // The case is about arithmetic at the top of the address space, so the two
    // pointers are synthesized at their addresses rather than offset from one
    // another: `add` on a pointer with no allocation behind it is UB in its own
    // right, which is what Miri reported here, and it is not the property under
    // test. `huge_mapping_suffix_from` only reads `.addr()`, so neither pointer
    // is ever dereferenced.
    let base = usize::MAX - 0x4000;
    let raw = core::ptr::without_provenance_mut::<u8>(base);
    unsafe {
        Segment::initialize(segment, raw, 0);
        (*segment).pages[0].block_size = 0x4000;
    }

    let user_ptr = core::ptr::without_provenance::<u8>(base + 0x1800);
    let suffix = unsafe { (*segment).huge_mapping_suffix_from(user_ptr) };

    assert_eq!(
        suffix, 0x2800,
        "huge suffix must avoid wrapping near the top of the address space"
    );
}
