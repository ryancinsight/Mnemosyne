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
//! [`prepare_measurement_host`] addresses both before the first sample is
//! taken, and reports what it actually achieved rather than assuming success:
//! a run that could not be prepared is a run whose numbers carry that caveat.

use core::fmt;

/// Result of restricting the process to the host's performance cores.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AffinityOutcome {
    /// The process is bound to `processors` performance cores.
    Bound {
        /// Number of logical processors in the bound set.
        processors: u32,
        /// Affinity mask the process was bound to.
        mask: usize,
    },
    /// The launcher already restricted the process to a set containing no
    /// performance core, so the operator's narrower choice is left in place.
    LauncherMaskPreserved {
        /// Mask the launcher supplied, left unchanged.
        mask: usize,
    },
    /// Every logical processor reports the same efficiency class, so the host
    /// has no performance-core subset to select.
    Homogeneous,
    /// The operating system rejected a query or the bind itself.
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
    OptedOut,
    /// The operating system rejected the request.
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
    use core::mem::{offset_of, size_of};

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
        /// `relationship` is the C `LOGICAL_PROCESSOR_RELATIONSHIP`
        /// enumeration, declared here as its unsigned four-byte
        /// representation so the record's `Relationship` field compares
        /// without a sign cast.
        fn GetLogicalProcessorInformationEx(
            relationship: u32,
            buffer: *mut c_void,
            returned_length: *mut u32,
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
    /// `RelationProcessorCore` in the `LOGICAL_PROCESSOR_RELATIONSHIP`
    /// enumeration.
    const RELATION_PROCESSOR_CORE: u32 = 0;
    /// `ERROR_INSUFFICIENT_BUFFER`, the expected result of the sizing call.
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

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

    /// Mirrors `GROUP_AFFINITY`.
    #[repr(C)]
    struct GroupAffinity {
        mask: usize,
        group: u16,
        reserved: [u16; 3],
    }

    /// Mirrors `PROCESSOR_RELATIONSHIP`. `group_mask` is declared
    /// `ANYSIZE_ARRAY` by Windows; the single element pins the offset of the
    /// first entry and the rest are read by stride.
    #[repr(C)]
    struct ProcessorRelationship {
        flags: u8,
        efficiency_class: u8,
        reserved: [u8; 20],
        group_count: u16,
        group_mask: [GroupAffinity; 1],
    }

    /// Mirrors `SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX` for the
    /// `RelationProcessorCore` union arm.
    #[repr(C)]
    struct ProcessorInformation {
        relationship: u32,
        size: u32,
        processor: ProcessorRelationship,
    }

    /// Field offsets inside one variable-length record, derived from the
    /// mirrored layouts rather than written as literals so a layout mistake is
    /// a compile error and never a silently misread efficiency class.
    const RELATIONSHIP_OFFSET: usize = offset_of!(ProcessorInformation, relationship);
    const SIZE_OFFSET: usize = offset_of!(ProcessorInformation, size);
    const EFFICIENCY_CLASS_OFFSET: usize = offset_of!(ProcessorInformation, processor)
        + offset_of!(ProcessorRelationship, efficiency_class);
    const GROUP_COUNT_OFFSET: usize = offset_of!(ProcessorInformation, processor)
        + offset_of!(ProcessorRelationship, group_count);
    const GROUP_MASK_OFFSET: usize =
        offset_of!(ProcessorInformation, processor) + offset_of!(ProcessorRelationship, group_mask);
    const GROUP_AFFINITY_STRIDE: usize = size_of::<GroupAffinity>();
    /// Smallest record this parser will read fields out of.
    const MINIMUM_RECORD_BYTES: usize = GROUP_MASK_OFFSET + GROUP_AFFINITY_STRIDE;

    const _: () = assert!(RELATIONSHIP_OFFSET == 0);
    const _: () = assert!(SIZE_OFFSET == 4);
    const _: () = assert!(EFFICIENCY_CLASS_OFFSET == 9);
    const _: () = assert!(GROUP_COUNT_OFFSET == 30);
    const _: () = assert!(GROUP_MASK_OFFSET == 32);
    const _: () = assert!(GROUP_AFFINITY_STRIDE == 16);

    /// `SetProcessAffinityMask` addresses one processor group, so only group
    /// zero's cores are candidates. Every host this harness runs on reports a
    /// single group; a multi-group host simply keeps its default placement.
    const AFFINITY_GROUP: u16 = 0;

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
        let records = match processor_core_records() {
            Ok(records) => records,
            Err(outcome) => return outcome,
        };
        let Some(top_class) = max_efficiency_class(&records) else {
            return AffinityOutcome::Homogeneous;
        };
        let performance_mask = mask_for_efficiency_class(&records, top_class);
        let process_mask = match current_process_mask() {
            Ok(mask) => mask,
            Err(outcome) => return outcome,
        };
        let selected = performance_mask & process_mask;
        if selected == 0 {
            return AffinityOutcome::LauncherMaskPreserved { mask: process_mask };
        }
        if selected == process_mask {
            // Nothing to narrow: either the host is homogeneous in practice or
            // the launcher already supplied exactly this set.
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

    /// One core's efficiency class and its group-zero affinity mask.
    struct CoreRecord {
        efficiency_class: u8,
        mask: usize,
    }

    /// Reads the host's `RelationProcessorCore` records.
    ///
    /// The buffer is allocated as `u64` so it carries the eight-byte alignment
    /// the record layout requires; `Vec<u8>` would only guarantee one.
    fn processor_core_records() -> Result<Vec<CoreRecord>, AffinityOutcome> {
        let mut length = 0u32;
        // SAFETY: documented Win32 sizing call; a null buffer with a zero
        // length is the documented way to ask for the required size.
        let sized = unsafe {
            GetLogicalProcessorInformationEx(
                RELATION_PROCESSOR_CORE,
                core::ptr::null_mut(),
                &raw mut length,
            )
        };
        // SAFETY: documented Win32 call taking no arguments.
        let sizing_error = unsafe { GetLastError() };
        if sized != 0 || sizing_error != ERROR_INSUFFICIENT_BUFFER || length == 0 {
            return Err(AffinityOutcome::Refused {
                operation: "GetLogicalProcessorInformationEx sizing",
                code: sizing_error,
            });
        }

        let words = (length as usize).div_ceil(size_of::<u64>());
        let mut storage = vec![0u64; words];
        // SAFETY: documented Win32 call. The buffer is `length` bytes reachable
        // from `storage`, eight-byte aligned by its `u64` element type, and
        // `length` is updated in place with the bytes actually written.
        let filled = unsafe {
            GetLogicalProcessorInformationEx(
                RELATION_PROCESSOR_CORE,
                storage.as_mut_ptr().cast(),
                &raw mut length,
            )
        };
        if filled == 0 {
            return Err(AffinityOutcome::Refused {
                operation: "GetLogicalProcessorInformationEx",
                // SAFETY: documented Win32 call taking no arguments.
                code: unsafe { GetLastError() },
            });
        }

        let bytes = storage.as_ptr().cast::<u8>();
        let written = (length as usize).min(words * size_of::<u64>());
        let mut records = Vec::new();
        let mut offset = 0usize;
        while offset + MINIMUM_RECORD_BYTES <= written {
            // SAFETY: `offset + MINIMUM_RECORD_BYTES <= written` and `written`
            // is within the allocation, so every field read below lies inside
            // the buffer the operating system filled.
            let (relationship, record_bytes, efficiency_class, group_count) = unsafe {
                (
                    read_u32(bytes, offset + RELATIONSHIP_OFFSET),
                    read_u32(bytes, offset + SIZE_OFFSET) as usize,
                    bytes.add(offset + EFFICIENCY_CLASS_OFFSET).read(),
                    read_u16(bytes, offset + GROUP_COUNT_OFFSET) as usize,
                )
            };
            if record_bytes < MINIMUM_RECORD_BYTES || offset + record_bytes > written {
                break;
            }
            if relationship == RELATION_PROCESSOR_CORE {
                for group in 0..group_count {
                    let entry = offset + GROUP_MASK_OFFSET + group * GROUP_AFFINITY_STRIDE;
                    if entry + GROUP_AFFINITY_STRIDE > offset + record_bytes {
                        break;
                    }
                    // SAFETY: `entry + GROUP_AFFINITY_STRIDE` is bounded by the
                    // record end, which the check above bounded by `written`.
                    let (mask, group_index) = unsafe {
                        (
                            read_usize(bytes, entry),
                            read_u16(bytes, entry + size_of::<usize>()),
                        )
                    };
                    if group_index == AFFINITY_GROUP {
                        records.push(CoreRecord {
                            efficiency_class,
                            mask,
                        });
                    }
                }
            }
            offset += record_bytes;
        }
        Ok(records)
    }

    /// # Safety
    ///
    /// `base + offset .. + 4` must lie inside the allocation behind `base`.
    unsafe fn read_u32(base: *const u8, offset: usize) -> u32 {
        // SAFETY: guaranteed by this function's contract.
        unsafe { base.add(offset).cast::<u32>().read_unaligned() }
    }

    /// # Safety
    ///
    /// `base + offset .. + 2` must lie inside the allocation behind `base`.
    unsafe fn read_u16(base: *const u8, offset: usize) -> u16 {
        // SAFETY: guaranteed by this function's contract.
        unsafe { base.add(offset).cast::<u16>().read_unaligned() }
    }

    /// # Safety
    ///
    /// `base + offset .. + size_of::<usize>()` must lie inside the allocation
    /// behind `base`.
    unsafe fn read_usize(base: *const u8, offset: usize) -> usize {
        // SAFETY: guaranteed by this function's contract.
        unsafe { base.add(offset).cast::<usize>().read_unaligned() }
    }

    /// The highest efficiency class present, or `None` when every core reports
    /// the same one (a homogeneous host has no performance subset to select).
    fn max_efficiency_class(records: &[CoreRecord]) -> Option<u8> {
        let mut classes = records.iter().map(|record| record.efficiency_class);
        let first = classes.next()?;
        let mut top = first;
        let mut mixed = false;
        for class in classes {
            mixed |= class != first;
            top = top.max(class);
        }
        mixed.then_some(top)
    }

    fn mask_for_efficiency_class(records: &[CoreRecord], class: u8) -> usize {
        records
            .iter()
            .filter(|record| record.efficiency_class == class)
            .fold(0usize, |mask, record| mask | record.mask)
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
