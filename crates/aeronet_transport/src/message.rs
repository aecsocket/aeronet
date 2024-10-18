use {
    crate::{lane::LaneIndex, TransportSet},
    aeronet_io::{packet::PacketBuffers, ringbuf::traits::Consumer},
    bevy_app::prelude::*,
    bevy_derive::{Deref, DerefMut},
    bevy_ecs::prelude::*,
    bevy_reflect::prelude::*,
    derive_more::{Add, AddAssign, Sub, SubAssign},
    octs::{BufMut, Bytes, BytesMut, Read},
    std::num::Saturating,
};

#[derive(Debug)]
pub(crate) struct MessagePlugin;

impl Plugin for MessagePlugin {
    fn build(&self, app: &mut App) {
        // TODO naive impl
        app.add_systems(PreUpdate, naive_poll.in_set(TransportSet::Poll))
            .add_systems(PostUpdate, naive_send.in_set(TransportSet::Flush));
    }
}

#[derive(Debug, Clone, Default, Component)]
pub struct MessageBuffers {
    pub recv: Vec<(LaneIndex, Bytes)>,
    pub send: MessageBuffersSend,
}

#[derive(Debug, Clone, Default)]
pub struct MessageBuffersSend {
    buf: Vec<(LaneIndex, Bytes)>,
}

impl MessageBuffersSend {
    pub fn push(&mut self, lane: impl Into<LaneIndex>, msg: Bytes) {
        let lane = lane.into();
        self.buf.push((lane, msg));
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Deref, DerefMut, Component, Reflect,
)]
#[reflect(Component)]
pub struct MessageMtu(pub usize);

#[derive(Debug, Clone, Copy, Component)]
#[doc(alias = "ping")]
#[doc(alias = "latency")]
pub struct MessageRtt {}

#[derive(Debug, Clone, Copy, Default, Component, Reflect, Add, AddAssign, Sub, SubAssign)]
#[reflect(Component)]
pub struct MessageStats {
    pub msgs_recv: Saturating<usize>,
    pub msgs_sent: Saturating<usize>,
    pub acks_recv: Saturating<usize>,
}

fn naive_poll(mut sessions: Query<(&mut PacketBuffers, &mut MessageBuffers)>) {
    for (mut packet_bufs, mut msg_bufs) in &mut sessions {
        let msgs = packet_bufs.recv.pop_iter().filter_map(|(_, mut packet)| {
            let lane_index = packet.read::<u32>().map(LaneIndex::from_raw).ok()?;
            Some((lane_index, packet))
        });
        msg_bufs.recv.extend(msgs);
    }
}

fn naive_send(mut sessions: Query<(&mut PacketBuffers, &mut MessageBuffers)>) {
    for (mut packet_bufs, mut msg_bufs) in &mut sessions {
        for (lane_index, msg) in msg_bufs.send.buf.drain(..) {
            let mut packet = BytesMut::new();
            packet.put_u32(lane_index.into_raw());
            packet.extend_from_slice(&msg);
            packet_bufs.send.push(packet.freeze());
        }
    }
}
