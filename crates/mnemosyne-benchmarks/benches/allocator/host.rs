//! Host controls that make one benchmark process's timings repeatable.
//!
//! Two host behaviours, not the allocator, dominated run-to-run spread on the
//! hybrid-core development host (see `benchmarks/allocator_baseline_metadata.md`,
//! MN-464):
//!
//! 1. **Core placement.** A `Core Ultra 9 285K` exposes eight performance cores
//!    and sixteen efficiency cores as one flat processor set. An unpinned
//!    benchmark thread is scheduled onto either class, so the same row measures
//!    two different machines depending on where it lands.
//! 2. **Power throttling.** Windows classifies a long-running, non-foreground
//!    process as background work and applies `EcoQoS`, capping execution speed
//!    for the whole process. A throttled run is uniformly three to five times
//!    slower than an unthrottled one and is indistinguishable, from the numbers
//!    alone, from a catastrophic allocator regression.
//!
//! Which cores are the performance cores is asked of [`themis::CpuTopology`],
//! not of the operating system here: the split is not inferable from processor
//! ids — this host's performance cores are the non-contiguous mask `0xc03c03` —
//! and one topology crate owning that query keeps every consumer on the same
//! answer. themis reports typed absence when a platform says nothing, and this
//! module preserves it rather than substituting a guess.
//!
//! [`prepare_measurement_host`] addresses both behaviours before the first
//! sample is taken, and reports what it actually achieved rather than assuming
//! success: a run that could not be prepared is a run whose numbers carry that
//! caveat.

use core::fmt;

/// Result of restricting the process to the host's performance cores.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AffinityOutcome {
    /// The process is bound to `processors` performance cores.
    #[cfg_attr(
        not(windows),
        expect(dead_code, reason = "constructed only by the Windows platform backend")
    )]
    Bound {
        /// Number of logical processors in the bound set.
        processors: u32,
        /// Affinity mask the process was bound to.
        mask: usize,
    },
    /// The launcher already restricted the process to a set containing no
    /// performance core, so the operator's narrower choice is left in place.
    #[cfg_attr(
        not(windows),
        expect(dead_code, reason = "constructed only by the Windows platform backend")
    )]
    LauncherMaskPreserved {
        /// Mask the launcher supplied, left unchanged.
        mask: usize,
    },
    /// Every logical processor reports the same efficiency class, so the host
    /// has no performance-core subset to select.
    #[cfg_attr(
        not(windows),
        expect(dead_code, reason = "constructed only by the Windows platform backend")
    )]
    Homogeneous,
    /// The platform reported no efficiency-class table at all, so there is no
    /// performance subset to select and none is guessed. Distinct from
    /// [`Self::Homogeneous`], which is a host that reported one class.
    #[cfg_attr(
        not(windows),
        expect(dead_code, reason = "constructed only by the Windows platform backend")
    )]
    ClassesUnreported,
    /// The operating system rejected a query or the bind itself.
    #[cfg_attr(
        not(windows),
        expect(dead_code, reason = "constructed only by the Windows platform backend")
    )]
    Refused {
        /// Platform operation that failed.
        operation: &'static str,
        /// Operating-system error code captured at the failure site.
        code: u32,
    },
    /// This target has no core-placement backend.
    #[cfg_attr(
        windows,
        expect(
            dead_code,
            reason = "constructed only by the non-Windows platform backend"
        )
    )]
    Unsupported,
}

/// Result of opting the process out of operating-system power throttling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThrottlingOutcome {
    /// Throttling is disabled for this process.
    #[cfg_attr(
        not(windows),
        expect(dead_code, reason = "constructed only by the Windows platform backend")
    )]
    OptedOut,
    /// The operating system rejected the request.
    #[cfg_attr(
        not(windows),
        expect(dead_code, reason = "constructed only by the Windows platform backend")
    )]
    Refused {
        /// Operating-system error code captured at the failure site.
        code: u32,
    },
    /// This target has no power-throttling backend.
    #[cfg_attr(
        windows,
        expect(
            dead_code,
            reason = "constructed only by the non-Windows platform backend"
        )
    )]
    Unsupported,
}

/// What [`prepare_measurement_host`] achieved for this process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostPreparation {
    /// Outcome of the power-throttling opt-out.
    pub throttling: ThrottlingOutcome,
    /// Outcome of the performance-core binding.
    pub affinity: AffinityOutcome,
}

impl fmt::Display for HostPreparation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.throttling {
            ThrottlingOutcome::OptedOut => formatter.write_str("power throttling opted out")?,
            ThrottlingOutcome::Refused { code } => {
                write!(formatter, "power-throttling opt-out REFUSED (error {code})")?;
            }
            ThrottlingOutcome::Unsupported => {
                formatter.write_str("power throttling not controllable on this target")?;
            }
        }
        formatter.write_str("; ")?;
        match self.affinity {
            AffinityOutcome::Bound { processors, mask } => write!(
                formatter,
                "bound to {processors} performance cores (mask {mask:#x})"
            ),
            AffinityOutcome::LauncherMaskPreserved { mask } => write!(
                formatter,
                "launcher affinity mask {mask:#x} preserved (contains no performance core)"
            ),
            AffinityOutcome::Homogeneous => {
                formatter.write_str("host cores are one efficiency class; no binding applied")
            }
            AffinityOutcome::ClassesUnreported => {
                formatter.write_str("host reported no efficiency classes; no binding applied")
            }
            AffinityOutcome::Refused { operation, code } => write!(
                formatter,
                "performance-core binding REFUSED at {operation} (error {code})"
            ),
            AffinityOutcome::Unsupported => {
                formatter.write_str("core placement not controllable on this target")
            }
        }
    }
}

/// Prepare this process for repeatable measurement.
///
/// Call once, before the first benchmark is registered. The returned report is
/// the run's provenance: numbers taken under a `REFUSED` outcome are not
/// comparable with numbers taken under a prepared one.
#[must_use]
pub fn prepare_measurement_host() -> HostPreparation {
    HostPreparation {
        throttling: platform::opt_out_of_power_throttling(),
        affinity: platform::bind_to_performance_cores(),
    }
}

#[cfg(windows)]
mod platform {
    use super::{AffinityOutcome, ThrottlingOutcome};
    use core::ffi::c_void;
    use core::mem::size_of;
    use themis::CpuTopology;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetLastError() -> u32;
        fn SetProcessInformation(
            process: isize,
            class: i32,
            information: *mut c_void,
            size: u32,
        ) -> i32;
        fn GetProcessAffinityMask(
            process: isize,
            process_mask: *mut usize,
            system_mask: *mut usize,
        ) -> i32;
        fn SetProcessAffinityMask(process: isize, mask: usize) -> i32;
    }

    /// `ProcessPowerThrottling` in the `PROCESS_INFORMATION_CLASS` enumeration.
    const PROCESS_POWER_THROTTLING: i32 = 4;
    /// `PROCESS_POWER_THROTTLING_EXECUTION_SPEED`: naming it in the control
    /// mask while leaving it clear in the state mask disables throttling
    /// outright rather than leaving the decision to the scheduler.
    const EXECUTION_SPEED: u32 = 0x1;

    #[repr(C)]
    struct PowerThrottlingState {
        version: u32,
        control_mask: u32,
        state_mask: u32,
    }

    /// Size Windows expects for `PROCESS_POWER_THROTTLING_STATE`, written as a
    /// `u32` literal and checked against the mirrored layout so the call needs
    /// no width cast.
    const POWER_THROTTLING_STATE_BYTES: u32 = 12;
    const _: () =
        assert!(size_of::<PowerThrottlingState>() == POWER_THROTTLING_STATE_BYTES as usize);

    pub(super) fn opt_out_of_power_throttling() -> ThrottlingOutcome {
        let mut state = PowerThrottlingState {
            version: 1,
            control_mask: EXECUTION_SPEED,
            state_mask: 0,
        };
        // SAFETY: documented Win32 call. The struct matches
        // `PROCESS_POWER_THROTTLING_STATE` and outlives the call, and the size
        // passed is that same struct's size.
        let applied = unsafe {
            SetProcessInformation(
                GetCurrentProcess(),
                PROCESS_POWER_THROTTLING,
                (&raw mut state).cast(),
                POWER_THROTTLING_STATE_BYTES,
            )
        };
        if applied == 0 {
            // SAFETY: documented Win32 call taking no arguments.
            ThrottlingOutcome::Refused {
                code: unsafe { GetLastError() },
            }
        } else {
            ThrottlingOutcome::OptedOut
        }
    }

    pub(super) fn bind_to_performance_cores() -> AffinityOutcome {
        let Some(topology) = CpuTopology::detect() else {
            return AffinityOutcome::ClassesUnreported;
        };
        match topology.is_hybrid() {
            // Typed absence: the platform did not report classes, so there is
            // no performance subset and this harness invents none.
            None => return AffinityOutcome::ClassesUnreported,
            Some(false) => return AffinityOutcome::Homogeneous,
            Some(true) => {}
        }
        let performance_mask = performance_core_mask(&topology);
        let process_mask = match current_process_mask() {
            Ok(mask) => mask,
            Err(outcome) => return outcome,
        };
        let selected = performance_mask & process_mask;
        if selected == 0 {
            return AffinityOutcome::LauncherMaskPreserved { mask: process_mask };
        }
        if selected == process_mask {
            // Nothing to narrow: the launcher already supplied exactly this set.
            return AffinityOutcome::Bound {
                processors: selected.count_ones(),
                mask: selected,
            };
        }
        // SAFETY: documented Win32 call; `selected` is a nonzero subset of the
        // mask the same process just reported.
        let applied = unsafe { SetProcessAffinityMask(GetCurrentProcess(), selected) };
        if applied == 0 {
            return AffinityOutcome::Refused {
                operation: "SetProcessAffinityMask",
                // SAFETY: documented Win32 call taking no arguments.
                code: unsafe { GetLastError() },
            };
        }
        AffinityOutcome::Bound {
            processors: selected.count_ones(),
            mask: selected,
        }
    }

    /// The affinity mask of the host's most performant core class.
    ///
    /// Zero when nothing of that class is nameable in an affinity mask, which
    /// `bind_to_performance_cores` reads through the same intersection it
    /// applies to the launcher's mask.
    ///
    /// The topology query itself is platform-independent; only the reduction to
    /// a mask is not. `SetProcessAffinityMask` addresses one processor group,
    /// and themis numbers processors `group * 64 + bit`, so a processor whose
    /// bit does not fit a `usize` belongs to a group this call cannot name --
    /// which is exactly the shift `checked_shl` declines.
    fn performance_core_mask(topology: &CpuTopology) -> usize {
        let Some(fastest) = topology.highest_efficiency_class() else {
            return 0;
        };
        let Some(processors) = topology.processors_in_efficiency_class(fastest) else {
            return 0;
        };
        processors
            .filter_map(|processor| 1usize.checked_shl(processor))
            .fold(0, |mask, bit| mask | bit)
    }

    /// This process's current affinity mask.
    ///
    /// Windows reports the system mask through the same call; it is written and
    /// discarded because the binding below may only narrow what the process
    /// already has.
    fn current_process_mask() -> Result<usize, AffinityOutcome> {
        let mut process_mask = 0usize;
        let mut discarded_system_mask = 0usize;
        // SAFETY: documented Win32 call writing through two owned locals.
        let queried = unsafe {
            GetProcessAffinityMask(
                GetCurrentProcess(),
                &raw mut process_mask,
                &raw mut discarded_system_mask,
            )
        };
        if queried == 0 {
            return Err(AffinityOutcome::Refused {
                operation: "GetProcessAffinityMask",
                // SAFETY: documented Win32 call taking no arguments.
                code: unsafe { GetLastError() },
            });
        }
        Ok(process_mask)
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{AffinityOutcome, ThrottlingOutcome};

    pub(super) fn opt_out_of_power_throttling() -> ThrottlingOutcome {
        ThrottlingOutcome::Unsupported
    }

    pub(super) fn bind_to_performance_cores() -> AffinityOutcome {
        AffinityOutcome::Unsupported
    }
}
