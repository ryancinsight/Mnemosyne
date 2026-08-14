# ADR 0002: Immutable WGPU callback pair

Status: superseded by ADR 0003 (2026-07-13)

## Context

`WgpuStagingBackend` publishes allocation and deallocation callbacks through
two independent atomics and documents re-registration. Neither property is
sound: readers can observe mixed generations, and a block allocated by one
generation can outlive registration of a different deallocator.

## Decision

Publish one `&'static WgpuCallbacks` through one `AtomicPtr`. Construction of
the immutable pair is unsafe and carries the allocation lifetime, pairing, and
no-unwind contract. Registration is safe and succeeds once; registering the
same static pair is idempotent, while a different pair returns a typed conflict
error. `WgpuStagingBackend` performs one Acquire load and invokes the selected
member. The successful compare-exchange uses Release publication, so a reader
that observes the pointer also observes both initialized function pointers.

Hephaestus owns one static pair and changes `WgpuDevice::new` to return its
existing typed `Result`, surfacing a competing process registration before it
publishes the staging device.

## Rejected alternatives

- Two atomics plus fences cannot bind allocation and deallocation lifetimes.
- A generation counter makes individual reads coherent but permits generation
  replacement while allocations remain live.
- Heap-allocating callback records introduces allocator recursion and permanent
  reclamation questions.
- `AtomicU128` is neither portable nor guaranteed lock-free and requires
  function-pointer integer representation assumptions.
- A lock adds hot-path contention and still cannot remain held for an
  allocation's lifetime.

## Failure modes and verification

Before registration, allocation returns null and deallocation returns false.
A competing static pair returns `WgpuCallbackRegistrationError`; the installed
pair remains unchanged. Local registry tests race two internally matching pairs
and assert that exactly one wins and that allocate/deallocate behavior never
mixes. Mnemosyne and Hephaestus package gates plus the real HostPinned contract
tests verify the coordinated boundary.
