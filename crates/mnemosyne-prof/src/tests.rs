#[test]
fn sample_metadata_stores_fixed_stack_id() {
    assert_eq!(
        core::mem::size_of::<crate::sampler::Sample>(),
        core::mem::size_of::<usize>() * 2,
        "sample metadata must store size plus fixed-width stack identity only"
    );
}
