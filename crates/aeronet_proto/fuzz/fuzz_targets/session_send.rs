#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|packet: &[u8]| {
    // TODO
});
