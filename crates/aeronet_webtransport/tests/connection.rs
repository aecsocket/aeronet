#![expect(missing_docs, reason = "testing")]
#![cfg(not(target_family = "wasm"))]
#![cfg_attr(
    not(target_family = "wasm"),
    expect(clippy::too_many_lines, reason = "testing")
)]

use {
    aeronet_io::{
        Session, SessionEndpoint,
        packet::RecvPacket,
        server::{Server, ServerEndpoint},
    },
    aeronet_webtransport::{
        client::{ClientConfig, WebTransportClient, WebTransportClientPlugin},
        server::{
            ServerConfig, SessionRequest, SessionResponse, WebTransportServer,
            WebTransportServerPlugin,
        },
    },
    bevy::prelude::*,
    bytes::Bytes,
    core::fmt::Debug,
    wtransport::Identity,
};

const PING: Bytes = Bytes::from_static(b"ping");
const PONG: Bytes = Bytes::from_static(b"pong");

#[test]
fn connect() {
    const PORT: u16 = 30000;

    _ = wtransport::tls::rustls::crypto::ring::default_provider().install_default();
    let identity = Identity::self_signed(["127.0.0.1", "::1", "localhost"]).unwrap();
    let cert_hash = identity.certificate_chain().as_slice()[0].hash();
    ping_pong(
        ServerConfig::builder()
            .with_bind_default(PORT)
            .with_identity(identity)
            .build(),
        ClientConfig::builder()
            .with_bind_default()
            .with_server_certificate_hashes([cert_hash])
            .build(),
        format!("https://[::1]:{PORT}"),
    );
}

// test harness

fn ping_pong(
    server_config: ServerConfig,
    client_config: ClientConfig,
    client_target: impl Into<String>,
) {
    #[derive(Debug, PartialEq, Eq)]
    enum ServerEvent {
        NewServerEndpoint,
        NewServer,
        NewClientEndpoint,
        NewClient,
        RecvPing,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum ClientEvent {
        NewSessionEndpoint,
        NewSession,
        RecvPong,
    }

    let mut server = {
        #[derive(Resource)]
        struct ServerEntity(Entity);

        #[derive(Resource)]
        struct ClientEntity(Entity);

        fn on_add_server_endpoint(
            trigger: Trigger<OnAdd, ServerEndpoint>,
            expected_server: Res<ServerEntity>,
            mut seq: ResMut<SequenceTester<ServerEvent>>,
        ) {
            assert_eq!(trigger.target(), expected_server.0);
            seq.event(ServerEvent::NewServerEndpoint).expect_first();
        }

        fn on_add_server(
            trigger: Trigger<OnAdd, Server>,
            expected_server: Res<ServerEntity>,
            mut seq: ResMut<SequenceTester<ServerEvent>>,
        ) {
            assert_eq!(trigger.target(), expected_server.0);
            seq.event(ServerEvent::NewServer)
                .expect_after(ServerEvent::NewServerEndpoint);
        }

        fn on_add_session_endpoint(
            trigger: Trigger<OnAdd, SessionEndpoint>,
            parents: Query<&ChildOf>,
            expected_server: Res<ServerEntity>,
            mut seq: ResMut<SequenceTester<ServerEvent>>,
            mut commands: Commands,
        ) {
            let client = trigger.target();
            let &ChildOf(server) = parents
                .get(client)
                .expect("parent server of client session should exist");
            assert_eq!(expected_server.0, server);
            seq.event(ServerEvent::NewClientEndpoint)
                .expect_after(ServerEvent::NewServer);
            commands.insert_resource(ClientEntity(client));
        }

        fn on_session_request(mut trigger: Trigger<SessionRequest>) {
            trigger.respond(SessionResponse::Accepted);
        }

        fn on_add_session(
            trigger: Trigger<OnAdd, Session>,
            expected_client: Res<ClientEntity>,
            mut seq: ResMut<SequenceTester<ServerEvent>>,
        ) {
            assert_eq!(expected_client.0, trigger.target());
            seq.event(ServerEvent::NewClient)
                .expect_after(ServerEvent::NewClientEndpoint);
        }

        fn recv_on_session(
            mut sessions: Query<&mut Session>,
            client: Option<Res<ClientEntity>>,
            mut seq: ResMut<SequenceTester<ServerEvent>>,
            mut exit: EventWriter<AppExit>,
        ) {
            let Some(client) = client else { return };
            let Ok(mut session) = sessions.get_mut(client.0) else {
                return;
            };
            let session = &mut *session;
            for RecvPacket {
                recv_at: _,
                payload,
            } in session.recv.drain(..)
            {
                if payload == PING {
                    seq.event(ServerEvent::RecvPing)
                        .expect_after(ServerEvent::NewClient);
                    session.send.push(PONG);
                    exit.write(AppExit::Success);
                }
            }
        }

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, WebTransportServerPlugin))
            .init_resource::<SequenceTester<ServerEvent>>()
            .add_observer(on_add_server_endpoint)
            .add_observer(on_add_server)
            .add_observer(on_add_session_endpoint)
            .add_observer(on_session_request)
            .add_observer(on_add_session)
            .add_systems(Update, recv_on_session);

        let world = app.world_mut();
        let server = world.spawn_empty().id();
        world.insert_resource(ServerEntity(server));
        WebTransportServer::open(server_config).apply(world.entity_mut(server));

        app
    };

    let mut client = {
        #[derive(Resource)]
        struct ClientEntity(Entity);

        fn on_add_session_endpoint(
            trigger: Trigger<OnAdd, SessionEndpoint>,
            mut seq: ResMut<SequenceTester<ClientEvent>>,
            mut commands: Commands,
        ) {
            let client = trigger.target();
            seq.event(ClientEvent::NewSessionEndpoint).expect_first();
            commands.insert_resource(ClientEntity(client));
        }

        fn on_add_session(
            trigger: Trigger<OnAdd, Session>,
            expected_client: Res<ClientEntity>,
            mut seq: ResMut<SequenceTester<ClientEvent>>,
            mut sessions: Query<&mut Session>,
        ) {
            let client = trigger.target();
            assert_eq!(expected_client.0, client);
            seq.event(ClientEvent::NewSession)
                .expect_after(ClientEvent::NewSessionEndpoint);
            let mut session = sessions
                .get_mut(client)
                .expect("target of trigger should exist");
            assert!(session.mtu() > PING.len());
            session.send.push(PING);
        }

        fn recv_on_session(
            mut sessions: Query<&mut Session>,
            client: Option<Res<ClientEntity>>,
            mut seq: ResMut<SequenceTester<ClientEvent>>,
            mut exit: EventWriter<AppExit>,
        ) {
            let Some(client) = client else { return };
            let Ok(mut session) = sessions.get_mut(client.0) else {
                return;
            };
            for RecvPacket {
                payload,
                recv_at: _,
            } in session.recv.drain(..)
            {
                if payload == PONG {
                    seq.event(ClientEvent::RecvPong)
                        .expect_after(ClientEvent::NewSession);
                    exit.write(AppExit::Success);
                }
            }
        }

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, WebTransportClientPlugin))
            .init_resource::<SequenceTester<ClientEvent>>()
            .add_observer(on_add_session_endpoint)
            .add_observer(on_add_session)
            .add_systems(Update, recv_on_session);

        let world = app.world_mut();
        let client = world.spawn_empty().id();
        WebTransportClient::connect(client_config, client_target.into())
            .apply(world.entity_mut(client));

        app
    };

    for _ in 0..10_000 {
        server.update();
        client.update();

        if server.should_exit() == Some(AppExit::Success)
            && client.should_exit() == Some(AppExit::Success)
        {
            return;
        }
    }

    panic!(
        "took too long to complete\n- server: {:?}\n- client: {:?}",
        server
            .world()
            .resource::<SequenceTester<ServerEvent>>()
            .events,
        client
            .world()
            .resource::<SequenceTester<ClientEvent>>()
            .events,
    );
}

#[derive(Debug, Resource)]
struct SequenceTester<E> {
    events: Vec<E>,
}

impl<E> Default for SequenceTester<E> {
    fn default() -> Self {
        Self { events: Vec::new() }
    }
}

impl<E: Debug + PartialEq> SequenceTester<E> {
    pub const fn event(&mut self, event: E) -> NextSequence<'_, E> {
        NextSequence {
            tester: self,
            next: event,
        }
    }
}

struct NextSequence<'t, E> {
    tester: &'t mut SequenceTester<E>,
    next: E,
}

impl<E: Debug + PartialEq> NextSequence<'_, E> {
    pub fn expect_first(self) {
        let next = self.next;
        assert!(
            self.tester.events.is_empty(),
            "expected first event to be {next:?}\nevent stack: {:?}",
            self.tester.events
        );
        self.tester.events.push(next);
    }

    pub fn expect_after(self, last: E) {
        let next = self.next;
        if let Some(our_last) = self.tester.events.last() {
            assert!(
                last == *our_last,
                "expected {last:?} then {next:?}, but was actually {our_last:?}\nevent stack: {:?}",
                self.tester.events,
            );
            self.tester.events.push(next);
        } else {
            panic!(
                "expected {last:?} then {next:?}, but this is the first event\nevent stack: {:?}",
                self.tester.events
            );
        }
    }
}
