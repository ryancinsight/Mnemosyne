use core::ffi::c_void;
use mnemosyne_core::constants::{MAX_ALLOC_SIZE, SEGMENT_SIZE};

const MAX_FUZZ_ALLOC: usize = 64 * 1024;
const MALLOC_ALIGN: usize = 16;

#[derive(Clone, Copy)]
struct ShimCase {
    op: u8,
    size: usize,
    nmemb: usize,
    alignment: usize,
}

impl ShimCase {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 25 {
            return None;
        }
        let op = data[0];
        let size = usize_from_le(&data[1..9]);
        let nmemb = usize_from_le(&data[9..17]);
        let alignment = usize_from_le(&data[17..25]);
        Some(Self {
            op,
            size,
            nmemb,
            alignment,
        })
    }
}

pub fn run(data: &[u8]) {
    let Some(case) = ShimCase::parse(data) else {
        return;
    };

    match case.op % 6 {
        0 => fuzz_malloc(case.size),
        1 => fuzz_calloc(case.nmemb, case.size),
        2 => fuzz_realloc(case.nmemb, case.size),
        3 => fuzz_aligned_alloc(case.alignment, case.size),
        4 => fuzz_posix_memalign(case.alignment, case.size),
        _ => fuzz_usable_size(case.size),
    }
}

fn fuzz_malloc(raw_size: usize) {
    let size = shaped_size(raw_size);
    let ptr = unsafe { mnemosyne_c_shim::malloc(size) };
    if ptr.is_null() {
        return;
    }
    assert_usable_at_least(ptr, malloc_request(size));
    write_request_edges(ptr, size);
    unsafe { mnemosyne_c_shim::free(ptr) };
}

fn fuzz_calloc(raw_nmemb: usize, raw_size: usize) {
    let (nmemb, size) = shaped_calloc_pair(raw_nmemb, raw_size);
    let ptr = unsafe { mnemosyne_c_shim::calloc(nmemb, size) };
    let total = nmemb.checked_mul(size);
    if total.is_none_or(|bytes| bytes > MAX_ALLOC_SIZE) {
        assert!(
            ptr.is_null(),
            "calloc overflow or oversized request succeeded"
        );
        return;
    }

    let request = malloc_request(total.unwrap());
    if ptr.is_null() {
        return;
    }
    assert_usable_at_least(ptr, request);
    assert_zero_prefix(ptr, total.unwrap().min(64));
    unsafe { mnemosyne_c_shim::free(ptr) };
}

fn fuzz_realloc(raw_old_size: usize, raw_new_size: usize) {
    let old_size = shaped_size(raw_old_size).min(MAX_FUZZ_ALLOC);
    let new_size = shaped_size(raw_new_size);
    let ptr = unsafe { mnemosyne_c_shim::malloc(malloc_request(old_size)) } as *mut u8;
    if ptr.is_null() {
        return;
    }

    let initialized = old_size.min(64);
    for index in 0..initialized {
        unsafe { ptr.add(index).write(pattern(index)) };
    }

    let resized = unsafe { mnemosyne_c_shim::realloc(ptr.cast::<c_void>(), new_size) } as *mut u8;
    if new_size == 0 {
        assert!(resized.is_null(), "realloc(_, 0) must return null");
        return;
    }
    if resized.is_null() {
        unsafe { mnemosyne_c_shim::free(ptr.cast::<c_void>()) };
        return;
    }

    let preserved = initialized.min(new_size);
    for index in 0..preserved {
        assert_eq!(
            unsafe { resized.add(index).read() },
            pattern(index),
            "realloc did not preserve initialized byte {index}"
        );
    }
    assert_usable_at_least(resized.cast::<c_void>(), malloc_request(new_size));
    unsafe { mnemosyne_c_shim::free(resized.cast::<c_void>()) };
}

fn fuzz_aligned_alloc(raw_alignment: usize, raw_size: usize) {
    let alignment = shaped_alignment(raw_alignment);
    let size = shaped_aligned_size(raw_size, alignment);
    let ptr = unsafe { mnemosyne_c_shim::aligned_alloc(alignment, size) };

    let valid_contract = alignment != 0
        && alignment.is_power_of_two()
        && size.is_multiple_of(alignment)
        && alignment <= SEGMENT_SIZE
        && malloc_request(size) <= MAX_ALLOC_SIZE;
    if !valid_contract {
        assert!(ptr.is_null(), "invalid aligned_alloc request succeeded");
        return;
    }
    if ptr.is_null() {
        return;
    }
    assert_eq!(
        ptr as usize % alignment,
        0,
        "aligned_alloc returned a misaligned pointer"
    );
    assert_usable_at_least(ptr, if size == 0 { alignment } else { size });
    write_request_edges(ptr, size);
    unsafe { mnemosyne_c_shim::free(ptr) };
}

fn fuzz_posix_memalign(raw_alignment: usize, raw_size: usize) {
    let alignment = shaped_alignment(raw_alignment);
    let size = shaped_size(raw_size);
    let mut out: *mut c_void = core::ptr::null_mut();
    let rc = unsafe { mnemosyne_c_shim::posix_memalign(&mut out, alignment, size) };

    let invalid_alignment =
        alignment < core::mem::size_of::<*mut c_void>() || !alignment.is_power_of_two();
    if invalid_alignment {
        assert_eq!(rc, 22, "invalid posix_memalign alignment must be EINVAL");
        assert!(out.is_null(), "posix_memalign touched memptr on EINVAL");
        return;
    }

    if alignment > SEGMENT_SIZE || malloc_request(size) > MAX_ALLOC_SIZE {
        assert_eq!(
            rc, 12,
            "unsupportable posix_memalign request must be ENOMEM"
        );
        assert!(out.is_null(), "posix_memalign touched memptr on ENOMEM");
        return;
    }

    if rc != 0 {
        assert!(out.is_null(), "posix_memalign failure touched memptr");
        return;
    }

    assert!(!out.is_null(), "posix_memalign success returned null");
    assert_eq!(
        out as usize % alignment,
        0,
        "posix_memalign returned a misaligned pointer"
    );
    assert_usable_at_least(out, if size == 0 { alignment } else { size });
    write_request_edges(out, size);
    unsafe { mnemosyne_c_shim::free(out) };
}

fn fuzz_usable_size(raw_size: usize) {
    assert_eq!(
        unsafe { mnemosyne_c_shim::malloc_usable_size(core::ptr::null_mut()) },
        0,
        "malloc_usable_size(NULL) must be 0"
    );

    let size = shaped_size(raw_size).min(MAX_FUZZ_ALLOC);
    let ptr = unsafe { mnemosyne_c_shim::malloc(size) };
    if ptr.is_null() {
        return;
    }
    assert_usable_at_least(ptr, malloc_request(size));
    unsafe { mnemosyne_c_shim::free(ptr) };
}

fn shaped_size(raw: usize) -> usize {
    match raw & 0x0f {
        0 => 0,
        1 => 1,
        2 => 7,
        3 => 8,
        4 => 15,
        5 => 16,
        6 => 64,
        7 => 4096,
        8 => 65_537,
        9 => SEGMENT_SIZE,
        10 => SEGMENT_SIZE + 1,
        11 => MAX_ALLOC_SIZE,
        12 => MAX_ALLOC_SIZE.saturating_add(1),
        13 => usize::MAX,
        _ => raw % (MAX_FUZZ_ALLOC + 1),
    }
}

fn shaped_calloc_pair(raw_nmemb: usize, raw_size: usize) -> (usize, usize) {
    match (raw_nmemb ^ raw_size) & 0x07 {
        0 => (0, shaped_size(raw_size)),
        1 => (1, shaped_size(raw_size)),
        2 => (2, shaped_size(raw_size).min(MAX_FUZZ_ALLOC / 2)),
        3 => (usize::MAX, 2),
        4 => (2, usize::MAX),
        5 => (MAX_ALLOC_SIZE, 2),
        6 => (MAX_FUZZ_ALLOC / 16, 16),
        _ => (
            raw_nmemb % 257,
            shaped_size(raw_size).min(MAX_FUZZ_ALLOC / 256),
        ),
    }
}

fn shaped_alignment(raw: usize) -> usize {
    match raw & 0x0f {
        0 => 0,
        1 => 1,
        2 => 3,
        3 => 8,
        4 => MALLOC_ALIGN,
        5 => 48,
        6 => 64,
        7 => 4096,
        8 => SEGMENT_SIZE,
        9 => SEGMENT_SIZE * 2,
        10 => usize::MAX,
        _ => 1usize << ((raw % 12) + 3),
    }
}

fn shaped_aligned_size(raw_size: usize, alignment: usize) -> usize {
    let size = shaped_size(raw_size);
    if alignment == 0 || !alignment.is_power_of_two() {
        return size;
    }
    match raw_size & 0x03 {
        0 => size - (size % alignment),
        1 => size.saturating_add(alignment - (size % alignment)),
        _ => size,
    }
}

fn malloc_request(size: usize) -> usize {
    if size == 0 {
        1
    } else {
        size
    }
}

fn assert_usable_at_least(ptr: *mut c_void, request: usize) {
    let usable = unsafe { mnemosyne_c_shim::malloc_usable_size(ptr) };
    assert!(
        usable >= request,
        "usable size {usable} under-reports request {request}"
    );
}

fn write_request_edges(ptr: *mut c_void, request: usize) {
    if request == 0 || request > MAX_FUZZ_ALLOC {
        return;
    }
    let bytes = ptr.cast::<u8>();
    unsafe {
        bytes.write(0xA5);
        bytes.add(request - 1).write(0x5A);
    }
}

fn assert_zero_prefix(ptr: *mut c_void, len: usize) {
    let bytes = ptr.cast::<u8>();
    for index in 0..len {
        assert_eq!(
            unsafe { bytes.add(index).read() },
            0,
            "calloc byte {index} was not zero-initialized"
        );
    }
}

fn pattern(index: usize) -> u8 {
    (index as u8).wrapping_mul(17).wrapping_add(0x31)
}

fn usize_from_le(bytes: &[u8]) -> usize {
    let mut value = 0usize;
    for (shift, byte) in bytes.iter().copied().enumerate() {
        value |= usize::from(byte) << (shift * 8);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executor_accepts_short_inputs_without_work() {
        run(&[0; 24]);
    }

    #[test]
    fn executor_drives_each_abi_operation_family() {
        for op in 0..6 {
            let mut data = [0_u8; 25];
            data[0] = op;
            data[1..9].copy_from_slice(&17_usize.to_le_bytes());
            data[9..17].copy_from_slice(&3_usize.to_le_bytes());
            data[17..25].copy_from_slice(&64_usize.to_le_bytes());
            run(&data);
        }
    }
}
