use aeronet::{
    client::{ClientTransport, ClientTransportPlugin, FromServer},
    lane::LaneIndex,
    server::{FromClient, ServerTransport, ServerTransportPlugin},
};
use aeronet_channel::{client::ChannelClient, server::ChannelServer};
use bevy::prelude::*;
use bytes::Bytes;

#[test]
fn send_recv() {
    const MSG1: Bytes = Bytes::from_static(b"hello 1");
    const MSG2: Bytes = Bytes::from_static(b"hello two");
    const LANE: LaneIndex = LaneIndex::from_raw(0);

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        ClientTransportPlugin::<ChannelClient>::default(),
        ServerTransportPlugin::<ChannelServer>::default(),
    ))
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            client_send_msg,
            server_recv_msg.run_if(on_event::<FromClient<ChannelServer>>()),
            client_recv_msg.run_if(on_event::<FromServer<ChannelClient>>()),
        )
            .chain(),
    );

    fn setup(mut commands: Commands) {
        let mut server = ChannelServer::open();
        let client = ChannelClient::connect_new(&mut server);
        commands.insert_resource(server);
        commands.insert_resource(client);
    }

    fn client_send_msg(mut client: ResMut<ChannelClient>) {
        client.send(MSG1, LANE).unwrap();
    }

    fn server_recv_msg(
        mut events: EventReader<FromClient<ChannelServer>>,
        mut server: ResMut<ChannelServer>,
    ) {
        let event = events.read().next().unwrap();
        assert_eq!(MSG1, event.msg);
        server.send(event.client_key, MSG2, LANE).unwrap();
    }

    fn client_recv_msg(mut events: EventReader<FromServer<ChannelClient>>) {
        let event = events.read().next().unwrap();
        assert_eq!(MSG2, event.msg);
    }

    app.update();
}
