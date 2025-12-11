#![no_main]

use {
    aeronet_transport::{
        Transport, TransportConfig,
        io::{Session, bytes::Bytes},
        lane::{LaneIndex, LaneKind},
    },
    libfuzzer_sys::fuzz_target,
    std::time::Instant,
};

fuzz_target!(|input: (LaneKind, &[u8])| {
    let (lane_kind, msg) = input;

    const MTU: usize = 128;
    const LANES: [LaneKind; 4] = [
        LaneKind::UnreliableUnordered,
        LaneKind::UnreliableSequenced,
        LaneKind::ReliableUnordered,
        LaneKind::ReliableOrdered,
    ];

    let now = Instant::now();
    let session = Session::new(now, MTU);
    let mut transport = Transport::new(&session, LANES, LANES, now).unwrap();

    let lane_index = LaneIndex::new(lane_kind as u32);
    let msg = Bytes::from(msg.to_vec());
    _ = transport.send.push(lane_index, msg, now).unwrap();

    let packets = aeronet_transport::send::flush_on(&mut transport, now, MTU).collect::<Vec<_>>();
    for packet in packets {
        aeronet_transport::recv::recv_on(&mut transport, &TransportConfig::default(), now, &packet)
            .unwrap();
    }
});
