//! Compile-time allocator behaviors and memory safety policies.
//!
//! # Zero-cost guarantee
//!
//! Every policy flag is a `const bool` associated item, not a runtime value.
//! Callers that specialize on `P::ENABLE_POISONING`, `P::ZERO_INITIALIZE`, etc.
//! will have those branches completely eliminated by the optimizer for
//! [`StandardPolicy`] (all false), leaving no overhead versus a hand-coded
//! non-poisoning allocator. The compile-time assertions in `impls` verify
//! this invariant at build time.
//!
//! # Module organization
//!
//! Every sub-module below is private; its public items are re-exported here.
//!
//! | Sub-module | Contents |
//! |------------|---------|
//! | [`mitigations`] | Compile-time bitmask constants for active mitigations |
//! | `private` | Sealed-trait infrastructure (not pub) |
//! | `alloc_policy` | `AllocPolicy` sealed trait definition |
//! | `impls` | `StandardPolicy`, `SecurePolicy`, `HardenedPolicy` ZSTs |
//! | `marker` | `PolicyMarker<P>` ZST typestate |

/// Sealed-trait infrastructure preventing out-of-crate `AllocPolicy` impls.
#[doc(hidden)]
pub mod private {
    pub trait Sealed {}
}

mod alloc_policy;
mod impls;
mod marker;
pub mod mitigations;
#[cfg(test)]
mod tests;

pub use alloc_policy::AllocPolicy;
pub use impls::{HardenedPolicy, SecurePolicy, StandardPolicy};
pub use marker::PolicyMarker;
