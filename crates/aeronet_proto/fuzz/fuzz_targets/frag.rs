#![no_main]

use aeronet_proto::frag::{FragmentHeader, FragmentReceiver};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: (FragmentHeader, &[u8])| {
    let (header, payload) = data;
    let _ = FragmentReceiver::new(1024, usize::MAX).reassemble(&header, payload);
});
