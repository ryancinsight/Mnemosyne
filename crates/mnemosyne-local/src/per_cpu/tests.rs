use super::state::NUM_CPU_SLOTS;
use super::types::PerCpuCacheHandle;

#[test]
fn cache_handle_allocates_storage_on_first_access() {
    let handle = PerCpuCacheHandle::new();
    assert_eq!(handle.storage.get().map(|cache| cache.slots.len()), None);

    let _cache = handle.get();

    assert_eq!(
        handle.storage.get().map(|cache| cache.slots.len()),
        Some(NUM_CPU_SLOTS)
    );
}
