#![no_main]

use std::time::Instant;

use aeronet::lane::LaneKind;
use aeronet_proto::session::{Session, SessionConfig};
use libfuzzer_sys::fuzz_target;

const MTU: usize = 1024;

fuzz_target!(|packet: &[u8]| {
    let config = SessionConfig::default().with_lanes([
        LaneKind::UnreliableUnordered,
        LaneKind::UnreliableSequenced,
        LaneKind::ReliableUnordered,
        LaneKind::ReliableOrdered,
    ]);

    let mut s = Session::new(Instant::now(), config, MTU, MTU).unwrap();
    let Ok((acks, msgs)) = s.recv(Instant::now(), packet.to_vec()) else {
        return;
    };

    for _ in acks {}

    msgs.for_each_msg(|_| {});
});
