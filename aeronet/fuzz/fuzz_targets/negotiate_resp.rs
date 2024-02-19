#![no_main]

use aeronet::{protocol::Negotiation, ProtocolVersion};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: [u8; Negotiation::RESPONSE_LEN]| {
    let _ = Negotiation::new(ProtocolVersion(0)).recv_response(&data);
});
