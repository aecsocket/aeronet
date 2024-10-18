#![no_main]

use std::time::Instant;

use aeronet_proto::{
    msg::FragmentReceiver,
    ty::{FragmentMarker, MessageSeq},
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: (usize, MessageSeq, FragmentMarker, &[u8])| {
    let (max_payload_len, msg_seq, marker, payload) = input;
    let max_payload_len = (max_payload_len % 1024).max(1);

    let mut r = FragmentReceiver::new(max_payload_len);
    _ = r.reassemble(Instant::now(), msg_seq, marker, payload);
});
