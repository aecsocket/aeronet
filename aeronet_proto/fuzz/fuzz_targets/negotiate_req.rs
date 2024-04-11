#![no_main]

use aeronet::protocol::ProtocolVersion;
use aeronet_proto::negotiate;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: [u8; negotiate::REQUEST_LEN]| {
    let _ = negotiate::Negotiation::new(ProtocolVersion(0)).recv_request(&data);
});
