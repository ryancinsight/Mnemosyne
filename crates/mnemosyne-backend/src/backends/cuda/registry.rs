//! Bounded lock-free pointer registries for live CUDA allocations.
//!
//! The CUDA driver owns deallocation, so the backends track only the live
//! pointers that must route back to the matching `cuMemFree*` entry point.
//! The fixed-size tables bound metadata without heap allocation. When a table
//! is full the owning backend releases the fresh driver allocation and
//! returns null from `allocate`; there is no host-allocation fallback.

use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// Capacity of each registry table. Registration fails (and the backend
/// returns null) once a table holds this many live pointers.
const MAX_TRACKED_CUDA_ALLOCATIONS: usize = 256;

/// A bounded registry for tracking active CUDA allocations.
pub struct CudaAllocationRegistry {
    slots: [AtomicPtr<u8>; MAX_TRACKED_CUDA_ALLOCATIONS],
    count: AtomicUsize,
}

impl CudaAllocationRegistry {
    /// Creates an empty registry with all slots free.
    const fn new() -> Self {
        Self {
            slots: [const { AtomicPtr::new(core::ptr::null_mut()) }; MAX_TRACKED_CUDA_ALLOCATIONS],
            count: AtomicUsize::new(0),
        }
    }
}

/// Live `cuMemAllocManaged` pointers owned by `CudaUnifiedBackend`.
pub(super) static CUDA_ALLOCATIONS: CudaAllocationRegistry = CudaAllocationRegistry::new();

/// Live `cuMemAllocManaged` (device-preferred) pointers owned by
/// `CudaDeviceBackend`.
pub(super) static CUDA_DEVICE_ALLOCATIONS: CudaAllocationRegistry = CudaAllocationRegistry::new();

/// Live `cuMemHostAlloc` pointers owned by `CudaHostPinnedBackend`.
pub(super) static CUDA_HOST_PINNED_ALLOCATIONS: CudaAllocationRegistry =
    CudaAllocationRegistry::new();

/// Records `ptr` in `registry`. Returns `false` when the table is full.
pub(super) fn register_cuda_ptr_in(registry: &CudaAllocationRegistry, ptr: *mut u8) -> bool {
    let start_idx = (ptr as usize >> 12) % MAX_TRACKED_CUDA_ALLOCATIONS;
    for i in 0..MAX_TRACKED_CUDA_ALLOCATIONS {
        let idx = (start_idx + i) % MAX_TRACKED_CUDA_ALLOCATIONS;
        let slot = &registry.slots[idx];
        // Double-check: cheap relaxed load avoids CAS invalidations on populated slots.
        if slot.load(Ordering::Relaxed).is_null()
            && slot
                .compare_exchange(
                    core::ptr::null_mut(),
                    ptr,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
        {
            registry.count.fetch_add(1, Ordering::Release);
            return true;
        }
    }
    false
}

/// Removes `ptr` from `registry`. Returns `false` when `ptr` is not present.
///
/// Scans the entire fixed-size table: the table is small and deallocation is
/// a cold path, so the full scan is bounded and cheap. Early-exit heuristics
/// based on a snapshot of `count` are unsound here — a concurrent
/// [`register_cuda_ptr_in`] can publish a non-null slot earlier in this
/// pointer's probe sequence after the snapshot was taken, exhausting the
/// budget before the target slot is reached; the failed unregistration would
/// then leak the CUDA allocation and permanently lose the slot.
pub(super) fn unregister_cuda_ptr_in(registry: &CudaAllocationRegistry, ptr: *mut u8) -> bool {
    let start_idx = (ptr as usize >> 12) % MAX_TRACKED_CUDA_ALLOCATIONS;
    for i in 0..MAX_TRACKED_CUDA_ALLOCATIONS {
        let idx = (start_idx + i) % MAX_TRACKED_CUDA_ALLOCATIONS;
        let slot = &registry.slots[idx];
        if slot.load(Ordering::Relaxed) == ptr
            && slot
                .compare_exchange(
                    ptr,
                    core::ptr::null_mut(),
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
        {
            registry.count.fetch_sub(1, Ordering::Release);
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_registry() -> CudaAllocationRegistry {
        CudaAllocationRegistry::new()
    }

    fn is_cuda_ptr_in(registry: &CudaAllocationRegistry, ptr: *mut u8) -> bool {
        let start_idx = (ptr as usize >> 12) % MAX_TRACKED_CUDA_ALLOCATIONS;
        for i in 0..MAX_TRACKED_CUDA_ALLOCATIONS {
            let idx = (start_idx + i) % MAX_TRACKED_CUDA_ALLOCATIONS;
            if registry.slots[idx].load(Ordering::Relaxed) == ptr {
                return true;
            }
        }
        false
    }

    /// One 4 KiB page of probe targets: all contained addresses share the same
    /// registry start index (`addr >> 12` is constant across the page), which
    /// makes probe-sequence collisions deterministic.
    #[repr(align(4096))]
    struct Page([u8; 4096]);

    #[test]
    fn cuda_registry_is_bounded_and_reusable() {
        let registry = test_registry();
        let mut bytes = [0_u8; MAX_TRACKED_CUDA_ALLOCATIONS + 1];

        for byte in bytes.iter_mut().take(MAX_TRACKED_CUDA_ALLOCATIONS) {
            assert!(register_cuda_ptr_in(&registry, byte as *mut u8));
        }

        assert!(!register_cuda_ptr_in(
            &registry,
            &mut bytes[MAX_TRACKED_CUDA_ALLOCATIONS] as *mut u8
        ));
        assert!(unregister_cuda_ptr_in(&registry, &mut bytes[7] as *mut u8));
        assert!(register_cuda_ptr_in(
            &registry,
            &mut bytes[MAX_TRACKED_CUDA_ALLOCATIONS] as *mut u8
        ));
    }

    #[test]
    fn cuda_registry_rejects_unknown_pointers() {
        let registry = test_registry();
        let mut byte = 0_u8;

        assert!(!unregister_cuda_ptr_in(&registry, &mut byte as *mut u8));
    }

    #[test]
    fn cuda_registry_hashing_and_fallback_forwarding() {
        let registry = test_registry();
        let mut byte1 = 0_u8;
        let mut byte2 = 0_u8;

        let ptr1 = &mut byte1 as *mut u8;
        let ptr2 = &mut byte2 as *mut u8;

        assert!(register_cuda_ptr_in(&registry, ptr1));
        assert!(is_cuda_ptr_in(&registry, ptr1));
        assert!(!is_cuda_ptr_in(&registry, ptr2));

        assert!(unregister_cuda_ptr_in(&registry, ptr1));
        assert!(!is_cuda_ptr_in(&registry, ptr1));
    }

    /// Regression test for the removed count-snapshot early exit.
    ///
    /// The old `unregister_cuda_ptr_in` loaded `count` once and stopped after
    /// seeing that many non-null slots. A register racing with the scan could
    /// publish its slot CAS earlier in the target's probe sequence before its
    /// `count` increment became part of the scanner's snapshot; the scan then
    /// exhausted its budget before reaching the target slot and returned
    /// `false`, leaking the CUDA allocation and the slot forever.
    ///
    /// The race window is reproduced deterministically: K pointers from one
    /// page (identical start index) are registered and then unregistered in
    /// front-to-back order so only the last, most-displaced pointer remains;
    /// a "racing" pointer is then placed directly into the vacated start slot
    /// *without* incrementing `count`, exactly the state a stale snapshot
    /// observes. The old heuristic saw one non-null slot (== stale count 1)
    /// and gave up 254 slots short of the target.
    #[test]
    fn cuda_registry_unregister_survives_register_race_and_reclaims_all_slots() {
        let registry = test_registry();
        let mut page = Page([0; 4096]);
        let base = page.0.as_mut_ptr();
        let start_idx = (base as usize >> 12) % MAX_TRACKED_CUDA_ALLOCATIONS;

        // Fill the whole table from one page: pointer k lands in slot
        // (start_idx + k) % MAX.
        let mut ptrs = [core::ptr::null_mut::<u8>(); MAX_TRACKED_CUDA_ALLOCATIONS];
        for (k, slot_ptr) in ptrs.iter_mut().enumerate() {
            // SAFETY: k < 4096, so the offset stays inside `page`.
            *slot_ptr = unsafe { base.add(k) };
            assert!(register_cuda_ptr_in(&registry, *slot_ptr));
        }
        assert_eq!(
            registry.count.load(Ordering::Acquire),
            MAX_TRACKED_CUDA_ALLOCATIONS
        );

        // Vacate everything except the last, most-displaced pointer.
        for &ptr in ptrs.iter().take(MAX_TRACKED_CUDA_ALLOCATIONS - 1) {
            assert!(unregister_cuda_ptr_in(&registry, ptr));
        }
        assert_eq!(registry.count.load(Ordering::Acquire), 1);
        let target = ptrs[MAX_TRACKED_CUDA_ALLOCATIONS - 1];

        // Simulate the racing register: its slot store is visible (start
        // slot, first probe position for `target`), its count increment is
        // not yet reflected in the scanner's snapshot.
        // SAFETY: offset 4095 stays inside `page`.
        let racer = unsafe { base.add(4095) };
        registry.slots[start_idx].store(racer, Ordering::Release);

        // Old heuristic: sees `racer` (1 non-null == stale count 1) at the
        // first probe position and breaks; the target slot is never reached.
        // The full scan must find and reclaim it.
        assert!(unregister_cuda_ptr_in(&registry, target));
        assert!(!is_cuda_ptr_in(&registry, target));

        // Complete the simulated racing register (its `count` increment
        // lands), then unregister it: every slot must be reclaimed.
        registry.count.fetch_add(1, Ordering::Release);
        assert!(unregister_cuda_ptr_in(&registry, racer));
        assert_eq!(registry.count.load(Ordering::Acquire), 0);

        // Value-semantic reclamation proof: a full table's worth of fresh
        // registrations succeeds again.
        for k in 0..MAX_TRACKED_CUDA_ALLOCATIONS {
            // SAFETY: k < 4096, so the offset stays inside `page`.
            assert!(register_cuda_ptr_in(&registry, unsafe { base.add(k) }));
        }
        assert_eq!(
            registry.count.load(Ordering::Acquire),
            MAX_TRACKED_CUDA_ALLOCATIONS
        );
    }
}
