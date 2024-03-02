#![no_main]

use aeronet_protocol::frag::{FragmentHeader, Fragmentation};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: (FragmentHeader, &[u8])| {
    let (header, payload) = data;
    let _ = Fragmentation::new(1024).reassemble(&header, payload);
});
