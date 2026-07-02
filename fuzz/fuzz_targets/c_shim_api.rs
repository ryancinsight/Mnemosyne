#![no_main]

use libfuzzer_sys::fuzz_target;

// The first input byte selects the executor mode (even = single-op,
// odd = op-sequence); see `mnemosyne_fuzz::c_shim_api::run`.
fuzz_target!(|data: &[u8]| {
    mnemosyne_fuzz::c_shim_api::run(data);
});
