#![no_main]

use aeronet_protocol::Fragmentation;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = Fragmentation::unsequenced().reassemble(data);
});
