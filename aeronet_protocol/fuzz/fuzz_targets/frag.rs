#![no_main]

use aeronet_protocol::{FragmentHeader, Fragmentation, FragmentationConfig, Seq};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: (Seq, FragmentHeader, &[u8])| {
    let (seq, header, payload) = data;
    let config = FragmentationConfig {
        payload_size: 1024,
        ..Default::default()
    };
    let _ = Fragmentation::new(&config).reassemble(seq, &header, payload);
});
