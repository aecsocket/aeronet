//! Testing purposes only.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use ringbuf::traits::Producer;

use crate::{
    io::PacketBuffers,
    transport::{MessageBuffers, TransportSet},
};

#[derive(Debug)]
pub struct NaiveTransportPlugin;

impl Plugin for NaiveTransportPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, poll.in_set(TransportSet::Poll))
            .add_systems(PostUpdate, flush.in_set(TransportSet::Flush));
    }
}

fn poll(mut sessions: Query<(&mut PacketBuffers, &mut MessageBuffers)>) {
    for (mut packet_bufs, mut msg_bufs) in &mut sessions {
        msg_bufs.recv.extend(packet_bufs.drain_recv());
    }
}

fn flush(mut sessions: Query<(&mut PacketBuffers, &mut MessageBuffers)>) {
    for (mut packet_bufs, mut msg_bufs) in &mut sessions {
        packet_bufs
            .send
            .push_iter(msg_bufs.send.drain(..).map(|(_, msg)| msg));
    }
}
