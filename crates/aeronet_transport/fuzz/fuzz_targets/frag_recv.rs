#![no_main]

use {
    aeronet_transport::{
        frag::FragmentReceiver,
        packet::{FragmentPosition, MessageSeq},
        size::MinSize,
    },
    libfuzzer_sys::fuzz_target,
};

fuzz_target!(|input: (FragmentPosition, MessageSeq, &[u8])| {
    let (position, msg_seq, payload) = input;

    const MAX_FRAG_LEN: MinSize = MinSize(32);
    // test with 8x the default memory limit
    // if we set it to `usize`, we can trivially crash the fuzzer
    // by sending a fragment with a large reported number of fragments
    const MEM_LEFT: usize = 8 * 4 * 1024 * 1024;

    let mut recv = FragmentReceiver::default();
    _ = recv.reassemble(
        MAX_FRAG_LEN,
        MEM_LEFT,
        MessageSeq::new(0),
        position,
        payload,
    );

    let mut recv = FragmentReceiver::default();
    _ = recv.reassemble(MAX_FRAG_LEN, MEM_LEFT, msg_seq, position, payload);
});
