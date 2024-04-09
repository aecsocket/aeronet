use aeronet::{
    client::{ClientTransport, ClientTransportPlugin, FromServer},
    message::Message,
    protocol::TransportProtocol,
    server::{FromClient, ServerTransport, ServerTransportPlugin},
};
use aeronet_channel::{client::ChannelClient, server::ChannelServer};
use bevy::prelude::*;

#[derive(Debug, Clone, Message)]
struct AppMessage(String);

impl<T: Into<String>> From<T> for AppMessage {
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

#[derive(Debug)]
struct AppProtocol;

impl TransportProtocol for AppProtocol {
    type C2S = AppMessage;
    type S2C = AppMessage;
}

type Client = ChannelClient<AppProtocol>;
type Server = ChannelServer<AppProtocol>;

#[test]
fn send_recv() {
    const MSG1: &str = "hello 1";
    const MSG2: &str = "hello two";

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        ClientTransportPlugin::<_, Client>::default(),
        ServerTransportPlugin::<_, Server>::default(),
    ))
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            client_send_msg,
            server_recv_msg.run_if(on_event::<FromClient<_, Server>>()),
            client_recv_msg.run_if(on_event::<FromServer<_, Client>>()),
        )
            .chain(),
    );

    fn setup(mut commands: Commands) {
        let mut server = Server::open();
        let client = Client::connect_new(&mut server);
        commands.insert_resource(server);
        commands.insert_resource(client);
    }

    fn client_send_msg(mut client: ResMut<Client>) {
        client.send(MSG1).unwrap();
    }

    fn server_recv_msg(
        mut events: EventReader<FromClient<AppProtocol, Server>>,
        mut server: ResMut<Server>,
    ) {
        let event = events.read().next().unwrap();
        assert_eq!(MSG1, event.msg.0);
        server.send(event.client_key, MSG2).unwrap();
    }

    fn client_recv_msg(mut events: EventReader<FromServer<AppProtocol, Client>>) {
        let event = events.read().next().unwrap();
        assert_eq!(MSG2, event.msg.0);
    }

    app.update();
}
