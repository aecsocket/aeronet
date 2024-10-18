use octs::Bytes;

use crate::lane::LaneIndex;

pub struct Seq(u16);

pub struct PacketSeq(pub Seq);

pub struct MessageSeq(pub Seq);

pub struct PacketHeader {
    pub seq: PacketSeq,
    pub acks: Acknowledge,
}

pub struct Acknowledge {
    pub last_recv: PacketSeq,
    pub bits: u32,
}

pub enum Frame {
    Message {
        lane: LaneIndex,
        seq: MessageSeq,
        payload: Bytes,
    },
    Fragment {
        lane: LaneIndex,
        seq: MessageSeq,
        marker: FragmentMarker,
        payload: Bytes,
    },
}

pub struct FragmentMarker(u8);

/*
packet examples:

```ignore
                           |--- frame ---------------| |--- frame ----------------| |--- frame --------------|
0x1234 0x1200 0b1111..1011 [ 0x0 0x50 b"hello world" ] [ 0x1 0x51 0x0 b"goodbye " ] [ 0x1 0x51 0x80 b"world" ]
^^^^^  ^^^^^^ ^^^^^^^^^^^^   ^^^ ^^^^ ^^^^^^^^^^^^^^     ^^^ ^^^  ^^^ ^^^^^^^^^^^     ^^^ ^^^^ ^^^^ ^^^^^^^^
| seq  |      | acks.bits    |   |    | payload          |   |    |   | payload       |   |    |    | payload
       | acks.last_recv      |   |                       |   |    | non-last frag     |   |    | last frag
                             |   | seq                   |   |    | index: 0          |   |    | index: 0
                             |                           |   | seq                    |   | seq
                             | lane: 0                   | lane: 0                    | lane: 0
                             | kind: Message             | kind: Fragment             | kind: Fragment
```

*/
