#[test]
fn capture_stack_stores_exact_retained_capacity() {
    let stack = crate::sampler::capture_stack();
    assert!(
        stack.len() <= 32,
        "captured stack length exceeded 32 frames: {}",
        stack.len()
    );
    assert_eq!(
        core::mem::size_of::<crate::sampler::Sample>(),
        core::mem::size_of::<usize>() + core::mem::size_of::<Box<[usize]>>(),
        "sample metadata must not retain a spare stack capacity word"
    );
}
