#![no_main]

use std::time::Instant;

use aeronet_proto::{
    msg::{FragmentReceiver, MessageSplitter, MAX_FRAGS},
    ty::MessageSeq,
};
use libfuzzer_sys::fuzz_target;

const MAX_PAYLOAD_LEN: usize = 1024;

fuzz_target!(|msg: &[u8]| {
    let s = MessageSplitter::new(MAX_PAYLOAD_LEN);
    let mut r = FragmentReceiver::new(MAX_PAYLOAD_LEN);

    let fs = s.split(msg.to_vec());
    if msg.len() > MAX_PAYLOAD_LEN * MAX_FRAGS {
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
