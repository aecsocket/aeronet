use {
    crate::{lane::LaneIndex, ProtoSet},
    aeronet_io::packet::PacketBuffers,
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
        app.add_systems(
            PreUpdate,
            (|mut q: Query<(&mut PacketBuffers, &mut MessageBuffers)>| {
                for (mut packet_bufs, mut msg_bufs) in &mut q {
                    let msgs = packet_bufs.drain_recv().filter_map(|mut packet| {
                        let lane_index = packet.read::<u64>().map(LaneIndex::from_raw).ok()?;
                        Some((lane_index, packet))
                    });
                    msg_bufs.recv.extend(msgs);
                }
            })
            .in_set(ProtoSet::Poll),
        )
        .add_systems(
            PostUpdate,
            (|mut q: Query<(&mut PacketBuffers, &mut MessageBuffers)>| {
                for (mut packet_bufs, mut msg_bufs) in &mut q {
                    for (lane_index, msg) in msg_bufs.send.drain(..) {
                        let mut packet = BytesMut::new();
                        packet.put_u64(lane_index.into_raw());
                        packet.extend_from_slice(&msg);
                        packet_bufs.push_send(packet.freeze());
                    }
                }
            })
            .in_set(ProtoSet::Flush),
        );
    }
}

#[derive(Debug, Clone, Default, Component)]
pub struct MessageBuffers {
    pub recv: Vec<(LaneIndex, Bytes)>,
    send: Vec<(LaneIndex, Bytes)>,
}

impl MessageBuffers {
    pub fn send(&mut self, lane_index: LaneIndex, msg: Bytes) {
        self.send.push((lane_index, msg));
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Deref, DerefMut, Component, Reflect,
)]
#[reflect(Component)]
pub struct MessageMtu(pub usize);

#[derive(Debug, Clone, Copy)]
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
