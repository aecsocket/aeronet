#![no_main]

use std::time::Instant;

use aeronet_proto::{
    msg::{FragmentReceiver, MessageSplitter, MAX_FRAGS},
    ty::MessageSeq,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: (usize, &[u8])| {
    let (max_payload_len, msg) = input;
    let max_payload_len = (max_payload_len % 1024).max(1);

    let s = MessageSplitter::new(max_payload_len);
    let mut r = FragmentReceiver::new(max_payload_len);

    let fs = s.split(msg.to_vec());
    if msg.len() > max_payload_len * MAX_FRAGS {
        assert!(fs.is_err());
        return;
    }

    let mut fs = fs.unwrap().peekable();
    while let Some((marker, payload)) = fs.next() {
        let res = r
            .reassemble(Instant::now(), MessageSeq::ZERO, marker, payload)
            .unwrap();
        if fs.peek().is_some() {
            assert!(res.is_none());
        } else {
            assert_eq!(msg, res.unwrap());
        }
    }
});
