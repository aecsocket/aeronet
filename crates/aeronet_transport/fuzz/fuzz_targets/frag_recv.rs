#![no_main]

use {
    aeronet_transport::{
        frag::FragmentReceiver,
        packet::{FragmentPosition, MessageSeq},
    },
    libfuzzer_sys::fuzz_target,
};

fuzz_target!(|input: (FragmentPosition, &[u8])| {
    let (position, payload) = input;

    const MAX_FRAG_LEN: usize = 32;
    const MEM_LEFT: usize = usize::MAX;
    const MSG_SEQ: MessageSeq = MessageSeq::new(0);

    let mut recv = FragmentReceiver::default();
    _ = recv.reassemble(MAX_FRAG_LEN, MEM_LEFT, MSG_SEQ, position, payload);
});
