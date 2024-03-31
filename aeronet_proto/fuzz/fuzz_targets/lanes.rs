#![no_main]

use aeronet_proto::Lanes;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = Lanes::new(1, &[]).recv(data);
});
