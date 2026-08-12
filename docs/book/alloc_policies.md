# Allocation Policies

Mnemosyne's behavior is controlled at compile time through the sealed
`AllocPolicy` trait, enabling zero-overhead behavioral customization.

## Built-In Policies

| Policy | Description |
|--------|-------------|
| `StandardPolicy` | Maximum performance. No poisoning, no zeroing. |
| `SecurePolicy` | Zero-initializes allocations; poisons freed memory; randomizes allocation order. |
| `HardenedPolicy` | Like `SecurePolicy` plus free-list pointer encryption. |

Each policy is a zero-sized type (ZST) with compile-time boolean constants:

| Constant | Description |
|----------|-------------|
| `ENABLE_POISONING` | Overwrite freed memory with a pattern |
| `ZERO_INITIALIZE` | Zero-fill allocations before returning |
| `ENABLE_FREE_LIST_ENCRYPTION` | XOR-encrypt free-list pointers |
| `RANDOMIZE_ALLOCATION` | Randomize slot selection order |

## Usage

```rust,ignore
use mnemosyne::{MnemosyneAllocator, SecurePolicy};

#[global_allocator]
static ALLOC: MnemosyneAllocator<SecurePolicy, _> = MnemosyneAllocator::new();
```

The default `Mnemosyne` unit struct uses `StandardPolicy` for production
workloads. Use `SecurePolicy` or `HardenedPolicy` in security-sensitive
or adversarial contexts.

## Backend Parameterization

`MnemosyneAllocator<P, B>` is generic over both policy `P` and backend `B`
(the memory mapping source). The same policy logic services CPU RAM, CUDA device
memory, HBM, and host-pinned CUDA memory without virtual dispatch.
