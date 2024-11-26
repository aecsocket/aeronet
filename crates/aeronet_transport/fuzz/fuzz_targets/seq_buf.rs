#![no_main]

use {aeronet_transport::seq_buf::SeqBuf, arbitrary::Arbitrary, libfuzzer_sys::fuzz_target};

#[derive(Debug, Arbitrary)]
enum OpKind {
    Insert,
    Remove,
}

#[derive(Debug, Arbitrary)]
struct Op {
    kind: OpKind,
    key: u16,
    value: u16,
}

fuzz_target!(|input: Box<[Op]>| {
    let mut buf = SeqBuf::<u16, 1024>::new();

    for op in input {
        match op.kind {
            OpKind::Insert => {
                buf.insert(op.key, op.value);
                let value = buf.get(op.key).unwrap();
                assert_eq!(op.value, *value);
            }
            OpKind::Remove => {
                buf.remove(op.key);
            }
        }
    }
});
