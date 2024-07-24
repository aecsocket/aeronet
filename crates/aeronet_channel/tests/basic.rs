use std::time::Duration;

use aeronet::{
    client::{ClientEvent, ClientTransport},
    lane::LaneIndex,
    server::{ServerEvent, ServerTransport},
};
use aeronet_channel::{client::ChannelClient, server::ChannelServer};
use assert_matches::assert_matches;
use bevy::prelude::*;
use bytes::Bytes;

#[test]
fn send_recv() {
    const MSG1: Bytes = Bytes::from_static(b"hello 1");
    const MSG2: Bytes = Bytes::from_static(b"hello two");
    const LANE: LaneIndex = LaneIndex::from_raw(0);

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (client_send_msg, server_recv_msg, client_recv_msg).chain(),
        );

    fn setup(mut commands: Commands) {
        let mut server = ChannelServer::new();
        server.open().unwrap();
        let mut client = ChannelClient::new();
        client.connect(&mut server).unwrap();
        commands.insert_resource(server);
        commands.insert_resource(client);
    }

    fn client_send_msg(mut client: ResMut<ChannelClient>) {
        client.send(MSG1, LANE).unwrap();
    }

    fn server_recv_msg(mut server: ResMut<ChannelServer>) {
        let mut events = server.poll(Duration::ZERO);
        let ServerEvent::Connecting { client_key: ck } = events.next().unwrap() else {
            panic!("expected Connecting");
        };
        assert_matches!(events.next().unwrap(), ServerEvent::Connected { client_key } if client_key == ck);
        assert_matches!(events.next().unwrap(), ServerEvent::Recv { client_key, msg, lane } if client_key == ck && msg == MSG1 && lane == LANE);
        assert!(events.next().is_none());

        drop(events);
        server.send(ck, MSG2, LANE).unwrap();
    }

    fn client_recv_msg(mut client: ResMut<ChannelClient>) {
        let mut events = client.poll(Duration::ZERO);
        assert_matches!(events.next().unwrap(), ClientEvent::Connected);
        assert_matches!(events.next().unwrap(), ClientEvent::Recv { msg, lane } if msg == MSG2 && lane == LANE);
        assert!(events.next().is_none());
    }

    app.update();
}
