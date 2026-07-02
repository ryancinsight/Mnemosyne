//! Kernel resource budgets for GPU occupancy planning (atlas ADR 0002).
//!
//! GPU register files and shared memory are **not host-allocatable**: the
//! kernel compiler assigns registers and kernels declare shared memory at
//! launch. Mnemosyne therefore owns the *budget vocabulary and accounting* —
//! how many registers per thread and shared bytes per block a kernel
//! requires — which moirai's occupancy planner intersects with per-unit
//! capacities (themis `GpuTopology` accessors: `registers_per_unit`,
//! `shared_mem_per_unit_bytes`, `max_threads_per_unit`) to derive launch
//! shapes. This module is `no_std`, dependency-free, and fully `const`:
//! every limiter resolves at compile time when the budget is a constant.

/// A kernel's per-launch resource requirements.
///
/// Constructed with [`KernelResourceBudget::new`], which rejects a zero
/// thread count (a launch with no threads is meaningless and would poison
/// the occupancy arithmetic). A `registers_per_thread` or
/// `shared_mem_per_block_bytes` of zero means the kernel uses none of that
/// resource and is therefore unconstrained by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KernelResourceBudget {
    registers_per_thread: u32,
    shared_mem_per_block_bytes: usize,
    threads_per_block: u32,
}

/// Result of intersecting a budget with one compute unit's capacities.
///
/// `u32::MAX` in a limiter means "unconstrained by this resource" — either
/// the kernel uses none of it, or the capacity is unreported (zero) and the
/// caller must decide policy rather than have a fabricated bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OccupancyLimits {
    /// Blocks per unit limited by the register file.
    pub by_registers: u32,
    /// Blocks per unit limited by shared memory.
    pub by_shared_mem: u32,
    /// Blocks per unit limited by resident-thread capacity.
    pub by_threads: u32,
}

impl OccupancyLimits {
    /// The binding constraint: the minimum of the three limiters.
    ///
    /// Returns `u32::MAX` only when every dimension is unconstrained, which
    /// the caller must treat as "no information", not "infinite blocks".
    #[must_use]
    #[inline]
    pub const fn blocks_per_unit(self) -> u32 {
        let mut limit = self.by_registers;
        if self.by_shared_mem < limit {
            limit = self.by_shared_mem;
        }
        if self.by_threads < limit {
            limit = self.by_threads;
        }
        limit
    }
}

impl KernelResourceBudget {
    /// Construct a budget. Returns `None` when `threads_per_block` is zero.
    #[must_use]
    pub const fn new(
        registers_per_thread: u32,
        shared_mem_per_block_bytes: usize,
        threads_per_block: u32,
    ) -> Option<Self> {
        if threads_per_block == 0 {
            return None;
        }
        Some(Self {
            registers_per_thread,
            shared_mem_per_block_bytes,
            threads_per_block,
        })
    }

    /// Registers each thread requires (compiler-reported).
    #[must_use]
    #[inline]
    pub const fn registers_per_thread(self) -> u32 {
        self.registers_per_thread
    }

    /// Shared-memory bytes each block declares at launch.
    #[must_use]
    #[inline]
    pub const fn shared_mem_per_block_bytes(self) -> usize {
        self.shared_mem_per_block_bytes
    }

    /// Threads per block of the planned launch shape.
    #[must_use]
    #[inline]
    pub const fn threads_per_block(self) -> u32 {
        self.threads_per_block
    }

    /// Registers one block consumes: `registers_per_thread · threads_per_block`.
    ///
    /// The product is computed in `u64` after widening both `u32` factors, so it
    /// is exact for every input: the maximum product `(2^32 - 1)^2 = 2^64 -
    /// 2^33 + 1` is strictly less than `u64::MAX`, so the multiplication never
    /// overflows and no saturation or wrapping is possible.
    #[must_use]
    #[inline]
    pub const fn registers_per_block(self) -> u64 {
        (self.registers_per_thread as u64) * (self.threads_per_block as u64)
    }

    /// Blocks per unit limited by a register file of `unit_registers`.
    ///
    /// Unconstrained (`u32::MAX`) when the kernel uses no registers or the
    /// capacity is unreported (zero) — an unreported capacity must surface as
    /// "no information" for the planner, never a fabricated bound.
    #[must_use]
    pub const fn blocks_limited_by_registers(self, unit_registers: u32) -> u32 {
        let per_block = self.registers_per_block();
        if per_block == 0 || unit_registers == 0 {
            return u32::MAX;
        }
        let blocks = (unit_registers as u64) / per_block;
        if blocks > u32::MAX as u64 {
            u32::MAX
        } else {
            blocks as u32
        }
    }

    /// Blocks per unit limited by `unit_shared_mem_bytes` of shared memory.
    /// Same unconstrained semantics as the register limiter.
    #[must_use]
    pub const fn blocks_limited_by_shared_mem(self, unit_shared_mem_bytes: usize) -> u32 {
        if self.shared_mem_per_block_bytes == 0 || unit_shared_mem_bytes == 0 {
            return u32::MAX;
        }
        let blocks = unit_shared_mem_bytes / self.shared_mem_per_block_bytes;
        if blocks > u32::MAX as usize {
            u32::MAX
        } else {
            blocks as u32
        }
    }

    /// Blocks per unit limited by `max_threads_per_unit` resident threads.
    /// Unconstrained when the capacity is unreported (zero).
    #[must_use]
    pub const fn blocks_limited_by_threads(self, max_threads_per_unit: u32) -> u32 {
        if max_threads_per_unit == 0 {
            return u32::MAX;
        }
        max_threads_per_unit / self.threads_per_block
    }

    /// Intersect this budget with one compute unit's capacities.
    ///
    /// The capacities are the themis `GpuTopology` per-unit accessors
    /// (`registers_per_unit()`, `shared_mem_per_unit_bytes()`,
    /// `max_threads_per_unit()`), passed as plain quantities so this crate
    /// stays `no_std` and dependency-free; the typed pairing lives in
    /// moirai's occupancy planner.
    #[must_use]
    pub const fn occupancy_limits(
        self,
        unit_registers: u32,
        unit_shared_mem_bytes: usize,
        max_threads_per_unit: u32,
    ) -> OccupancyLimits {
        OccupancyLimits {
            by_registers: self.blocks_limited_by_registers(unit_registers),
            by_shared_mem: self.blocks_limited_by_shared_mem(unit_shared_mem_bytes),
            by_threads: self.blocks_limited_by_threads(max_threads_per_unit),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ampere-class unit capacities used as closed-form fixtures.
    const UNIT_REGISTERS: u32 = 65_536;
    const UNIT_SHARED: usize = 102_400;
    const UNIT_THREADS: u32 = 1_536;

    #[test]
    fn zero_thread_budget_is_rejected() {
        assert!(KernelResourceBudget::new(32, 0, 0).is_none());
        assert!(KernelResourceBudget::new(32, 0, 1).is_some());
    }

    #[test]
    fn register_limiter_matches_closed_form() {
        // 64 regs/thread × 256 threads = 16384 regs/block; 65536/16384 = 4.
        let budget = KernelResourceBudget::new(64, 0, 256).unwrap();
        assert_eq!(budget.registers_per_block(), 16_384);
        assert_eq!(budget.blocks_limited_by_registers(UNIT_REGISTERS), 4);
    }

    #[test]
    fn shared_mem_limiter_matches_closed_form() {
        // 100 KiB unit / 16 KiB per block = 6 blocks (floor).
        let budget = KernelResourceBudget::new(0, 16 * 1024, 128).unwrap();
        assert_eq!(budget.blocks_limited_by_shared_mem(UNIT_SHARED), 6);
    }

    #[test]
    fn thread_limiter_matches_closed_form() {
        // 1536 resident / 256 per block = 6 blocks.
        let budget = KernelResourceBudget::new(0, 0, 256).unwrap();
        assert_eq!(budget.blocks_limited_by_threads(UNIT_THREADS), 6);
    }

    #[test]
    fn binding_constraint_is_the_minimum() {
        // Registers bind at 4, shared at 6, threads at 6 -> 4.
        let budget = KernelResourceBudget::new(64, 16 * 1024, 256).unwrap();
        let limits = budget.occupancy_limits(UNIT_REGISTERS, UNIT_SHARED, UNIT_THREADS);
        assert_eq!(limits.by_registers, 4);
        assert_eq!(limits.by_shared_mem, 6);
        assert_eq!(limits.by_threads, 6);
        assert_eq!(limits.blocks_per_unit(), 4);
    }

    #[test]
    fn unreported_capacities_are_unconstrained_not_fabricated() {
        let budget = KernelResourceBudget::new(64, 16 * 1024, 256).unwrap();
        let limits = budget.occupancy_limits(0, 0, 0);
        assert_eq!(limits.by_registers, u32::MAX);
        assert_eq!(limits.by_shared_mem, u32::MAX);
        assert_eq!(limits.by_threads, u32::MAX);
        assert_eq!(limits.blocks_per_unit(), u32::MAX);
    }

    #[test]
    fn zero_resource_budgets_are_unconstrained_by_that_resource() {
        let budget = KernelResourceBudget::new(0, 0, 256).unwrap();
        let limits = budget.occupancy_limits(UNIT_REGISTERS, UNIT_SHARED, UNIT_THREADS);
        assert_eq!(limits.by_registers, u32::MAX);
        assert_eq!(limits.by_shared_mem, u32::MAX);
        assert_eq!(limits.blocks_per_unit(), 6); // threads bind
    }

    #[test]
    fn limits_are_const_evaluable() {
        // The whole pipeline resolves at compile time for constant budgets.
        const BUDGET: KernelResourceBudget = match KernelResourceBudget::new(64, 16 * 1024, 256) {
            Some(budget) => budget,
            None => panic!("non-zero thread count"),
        };
        const LIMITS: OccupancyLimits =
            BUDGET.occupancy_limits(UNIT_REGISTERS, UNIT_SHARED, UNIT_THREADS);
        const BLOCKS: u32 = LIMITS.blocks_per_unit();
        assert_eq!(BLOCKS, 4);
    }
}
