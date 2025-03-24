#![no_main]

use {
    aeronet_transport::{Transport, io::Session, lane::LaneKind},
    libfuzzer_sys::fuzz_target,
    std::time::Instant,
};

fuzz_target!(|packet: &[u8]| {
    const MTU: usize = 128;
    const RECV_LANES: [LaneKind; 4] = [
        LaneKind::UnreliableUnordered,
        LaneKind::UnreliableSequenced,
        LaneKind::ReliableUnordered,
        LaneKind::ReliableOrdered,
    ];

    let now = Instant::now();
    let session = Session::new(now, MTU);
    let mut transport = Transport::new(&session, RECV_LANES, [], now).unwrap();
    _ = aeronet_transport::recv::fuzz_recv_on(&mut transport, packet);
});
