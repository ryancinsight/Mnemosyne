use crate::MnemosyneOptions;

/// Returns the current allocator configuration options snapshot.
#[inline]
pub fn get_options() -> MnemosyneOptions {
    mnemosyne_core::options::get_options()
}

/// Configures the allocator runtime settings programmatically.
///
/// Modifies the global settings. Can be called at runtime; changes apply
/// to subsequent allocator operations. If the purge cadence is changed
/// to a non-zero value and background purger was inactive, starts the
/// background decay engine thread.
#[inline]
pub fn configure(options: MnemosyneOptions) {
    let old_cadence =
        mnemosyne_core::options::PURGE_CADENCE_MS.load(core::sync::atomic::Ordering::Acquire);
    mnemosyne_core::options::set_options(options);
    mnemosyne_local::mark_options_initialized();

    if options.purge_cadence_ms > 0 && old_cadence == 0 {
        mnemosyne_decay::init_decay_engine();
    }
}
