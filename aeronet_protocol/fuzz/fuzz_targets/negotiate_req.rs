#![no_main]

use aeronet::ProtocolVersion;
use aeronet_protocol::Negotiation;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: [u8; Negotiation::REQUEST_LEN]| {
    let _ = Negotiation::new(ProtocolVersion(0)).recv_request(&data);
});
