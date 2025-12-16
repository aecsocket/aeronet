#![expect(missing_docs, reason = "testing")]
#![cfg(test)]
#![cfg(not(target_family = "wasm"))]

use {
    aeronet_io::Session,
    aeronet_websocket::{
        client::{ClientConfig, WebSocketClient, WebSocketClientPlugin},
        server::{ServerConfig, WebSocketServer, WebSocketServerPlugin},
    },
    bevy::prelude::*,
    bytes::Bytes,
};

#[test]
fn exchange_string() {
    test_exchange(29100, b"hello world");
}

#[test]
fn exchange_empty() {
    test_exchange(29101, b"");
}

fn test_exchange(port: u16, msg: &'static [u8]) {
    let mut server_app = {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, WebSocketServerPlugin))
            .add_systems(
                Update,
                move |mut sessions: Query<&mut Session>, mut exit: MessageWriter<AppExit>| {
                    for mut session in &mut sessions {
                        if let Some(packet) = session.recv.drain(..).next() {
                            assert_eq!(&packet.payload, msg);
                            exit.write(AppExit::Success);
                        }
                    }
                },
            )
            .add_observer(
                |trigger: On<Add, Session>, mut session: Query<&mut Session>| {
                    let mut session = session.get_mut(trigger.entity).unwrap();
                    session.send.push(Bytes::new());
                },
            );

        let entity = app.world_mut().spawn_empty();
        WebSocketServer::open(
            ServerConfig::builder()
                .with_bind_default(port)
                .with_no_encryption(),
        )
        .apply(entity);

        app
    };

    let mut client_app = {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, WebSocketClientPlugin));

        let mut entity = app.world_mut().spawn_empty();

        // send packet on connect
        entity.observe(
            move |trigger: On<Add, Session>, mut session: Query<&mut Session>| {
                let mut session = session.get_mut(trigger.entity).unwrap();
                session.send.push(Bytes::from_static(msg));
            },
        );

        // connect to server
        WebSocketClient::connect(
            ClientConfig::builder().with_no_encryption(),
            format!("ws://[::1]:{port}"),
        )
        .apply(entity);

        app
    };

    for _ in 0..10_000 {
        server_app.update();
        client_app.update();

        if server_app.should_exit() == Some(AppExit::Success) {
            return;
        }
    }

    panic!("took too long to complete");
}
