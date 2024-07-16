#![no_main]

use aeronet::lane::LaneKind;
use aeronet_proto::messages::Messages;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut msgs = Messages::new(1, 1, [LaneKind::UnreliableUnordered]);
    // TODO
});
