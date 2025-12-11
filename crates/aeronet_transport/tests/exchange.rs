#![expect(missing_docs, reason = "testing")]
#![cfg(test)]

use {
    aeronet_io::{
        AeronetIoPlugin, Session,
        packet::{NoClearBuffers, RecvPacket},
    },
    aeronet_transport::{
        AeronetTransportPlugin, Transport,
        lane::{LaneIndex, LaneKind},
    },
    bevy_app::prelude::*,
    bevy_platform::time::Instant,
    bevy_time::TimePlugin,
    octs::Bytes,
};

const LANES: [LaneKind; 1] = [LaneKind::ReliableOrdered];
const LANE: LaneIndex = LaneIndex::new(0);

#[test]
fn simple() {
    round_trip(b"hello world");
}

#[test]
fn empty() {
    round_trip(b"");
}

fn round_trip(msg: &'static [u8]) {
    let mut app = App::new();
    app.add_plugins((TimePlugin, AeronetIoPlugin, AeronetTransportPlugin))
        .insert_resource(NoClearBuffers);

    let now = Instant::now();
    let session = Session::new(now, 1024);
    let mut transport = Transport::new(&session, LANES, LANES, now).unwrap();
    transport
        .send
        .push(LANE, Bytes::from_static(msg), now)
        .unwrap();
    assert_eq!(1, transport.send.lanes().first().unwrap().num_queued_msgs());
    let entity = app.world_mut().spawn((session, transport)).id();

    app.update();

    // transport message has been flushed to session
    // take the sent packet and re-insert it as a received packet

    // // the message will still be queued until the next time we update
    // // since we've removed the fragment for sending *after* dropping messages with no fragments
    // let transport = app.world().get::<Transport>(entity).unwrap();
    // assert_eq!(0, transport.send.lanes().first().unwrap().num_queued_msgs());

    let mut session = app.world_mut().get_mut::<Session>(entity).unwrap();
    assert_eq!(1, session.send.len());
    let mut packets = session.send.drain(..);
    let packet = packets.next().unwrap();

    assert!(packets.next().is_none());
    drop(packets);
    session.recv.push(RecvPacket {
        recv_at: now,
        payload: packet,
    });

    app.update();

    let mut transport = app.world_mut().get_mut::<Transport>(entity).unwrap();
    let mut msgs = transport.recv.msgs.drain();
    assert!(msgs.next().is_some());
    assert!(msgs.next().is_none());
}
