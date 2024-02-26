#![no_main]

use aeronet_protocol::{FragmentHeader, Fragmentation, FragmentationConfig, Seq};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: (Seq, FragmentHeader, &[u8])| {
    let (seq, header, payload) = data;
    let _ = Fragmentation::new(1024, 256).reassemble(seq, &header, payload);
});
