#![expect(missing_docs, reason = "testing")]

use std::{fmt::Debug, thread, time::Duration};

use aeronet_io::{
    Session, SessionEndpoint,
    server::{Server, ServerEndpoint},
};
use aeronet_websocket::{
    client::{ClientConfig, WebSocketClient, WebSocketClientPlugin},
    server::{ServerConfig, WebSocketServer, WebSocketServerPlugin},
};
use bevy::prelude::*;

#[test]
fn connect_unencrypted() {
    #[derive(Debug, PartialEq, Eq)]
    enum TestEvent {
        NewServerEndpoint,
        NewServer,
        NewClientEndpoint,
        NewClient,
    }

    const PORT: u16 = 29000;

    let server = thread::spawn(move || {
        #[derive(Resource)]
        struct ServerEntity(Entity);

        #[derive(Resource)]
        struct ClientEntity(Entity);

        fn setup(mut commands: Commands) {
            let server = commands.spawn_empty().id();
            commands.insert_resource(ServerEntity(server));
            commands.entity(server).queue(WebSocketServer::open(
                ServerConfig::builder()
                    .with_bind_default(PORT)
                    .with_no_encryption(),
            ));
        }

        fn on_add_server_endpoint(
            trigger: Trigger<OnAdd, ServerEndpoint>,
            expected_server: Res<ServerEntity>,
            mut seq: ResMut<AssertSequence<TestEvent>>,
        ) {
            assert_eq!(trigger.entity(), expected_server.0);
            seq.firstly(TestEvent::NewServerEndpoint);
        }

        fn on_add_server(
            trigger: Trigger<OnAdd, Server>,
            expected_server: Res<ServerEntity>,
            mut seq: ResMut<AssertSequence<TestEvent>>,
        ) {
            assert_eq!(trigger.entity(), expected_server.0);
            seq.after(TestEvent::NewServerEndpoint, TestEvent::NewServer);
        }

        fn on_add_session_endpoint(
            trigger: Trigger<OnAdd, SessionEndpoint>,
            parents: Query<&Parent>,
            expected_server: Res<ServerEntity>,
            mut seq: ResMut<AssertSequence<TestEvent>>,
            mut commands: Commands,
        ) {
            let client = trigger.entity();
            let parent = parents.get(client).map(Parent::get).unwrap();
            assert_eq!(expected_server.0, parent);
            seq.after(TestEvent::NewServer, TestEvent::NewClientEndpoint);
            commands.insert_resource(ClientEntity(client));
        }

        fn on_add_session(
            trigger: Trigger<OnAdd, Session>,
            expected_client: Res<ClientEntity>,
            mut seq: ResMut<AssertSequence<TestEvent>>,
        ) {
            assert_eq!(expected_client.0, trigger.entity());
            seq.after(TestEvent::NewClientEndpoint, TestEvent::NewClient);
            println!("success!");
        }

        App::new()
            .add_plugins((MinimalPlugins, WebSocketServerPlugin))
            .init_resource::<AssertSequence<TestEvent>>()
            .add_systems(Startup, setup)
            .add_systems(Update, panic_if_running_too_long)
            .add_observer(on_add_server_endpoint)
            .add_observer(on_add_server)
            .add_observer(on_add_session_endpoint)
            .add_observer(on_add_session)
            .run();
    });

    let client = thread::spawn(move || {
        App::new()
            .add_plugins((MinimalPlugins, WebSocketClientPlugin))
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn_empty().queue(WebSocketClient::connect(
                    ClientConfig::builder().with_no_encryption(),
                    format!("ws://[::1]:{PORT}"),
                ));
            })
            .add_systems(Update, panic_if_running_too_long)
            .run();
    });

    server.join().unwrap();
    client.join().unwrap();
}

// test harness

fn panic_if_running_too_long(time: Res<Time<Real>>) {
    assert!(
        time.elapsed() < Duration::from_millis(500),
        "took too long to complete"
    );
}

#[derive(Debug, Resource)]
struct AssertSequence<E> {
    prev: Option<E>,
}

impl<E> Default for AssertSequence<E> {
    fn default() -> Self {
        Self { prev: None }
    }
}

impl<E: Debug + PartialEq> AssertSequence<E> {
    fn firstly(&mut self, event: E) {
        if let Some(prev) = &self.prev {
            panic!("expected {prev:?} first, but {event:?} already happened");
        }
        self.prev = Some(event);
    }

    fn after(&mut self, prev: E, next: E) {
        if let Some(our_prev) = &self.prev {
            assert!(
                prev == *our_prev,
                "expected {prev:?} then {next:?}, but previous event was {our_prev:?}"
            );
            self.prev = Some(prev);
        } else {
            panic!("expected {prev:?} then {next:?}, but no event has happened yet");
        }
    }
}
