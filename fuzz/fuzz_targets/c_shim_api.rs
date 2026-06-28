#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    mnemosyne_fuzz::c_shim_api::run(data);
});
