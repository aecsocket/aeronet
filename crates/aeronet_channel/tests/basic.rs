use std::time::Duration;

use aeronet::{
    client::{ClientEvent, ClientTransport, DisconnectReason},
    lane::LaneIndex,
    server::{CloseReason, ServerEvent, ServerTransport},
    shared::DROP_DISCONNECT_REASON,
};
use aeronet_channel::{
    client::ChannelClient,
    server::{ChannelServer, ClientKey},
};
use assert_matches::assert_matches;

const C2S: &[u8] = b"hello server";
const S2C: &[u8] = b"hello client";

const LANE: LaneIndex = LaneIndex::from_raw(0);
const DT: Duration = Duration::ZERO;

const REASON: &str = "disconnection reason here";

fn open() -> (ChannelClient, ChannelServer, ClientKey) {
    let mut server = ChannelServer::new();
    server.open().unwrap();
    let mut client = ChannelClient::new();
    client.connect(&mut server).unwrap();

    {
        let mut events = client.poll(DT);
        assert_matches!(events.next().unwrap(), ClientEvent::Connected);
        assert!(events.next().is_none());
    }

    let target_key = {
        let mut events = server.poll(DT);

        let ServerEvent::Connecting {
            client_key: target_key,
        } = events.next().unwrap()
        else {
            panic!("expected Connecting");
        };
        assert_matches!(
            events.next().unwrap(),
            ServerEvent::Connected { client_key } if client_key == target_key
        );
        assert!(events.next().is_none());

        target_key
    };

    (client, server, target_key)
}

#[test]
fn send_recv() {
    let (mut client, mut server, target_key) = open();

    client.send(C2S, LANE).unwrap();

    assert!(client.poll(DT).next().is_none());

    {
        let mut events = server.poll(DT);
        assert_matches!(
            events.next().unwrap(),
            ServerEvent::Recv { client_key, msg, lane } if client_key == target_key && msg == C2S && lane == LANE
        );
        assert!(events.next().is_none());
    }

    server.send(target_key, S2C, LANE).unwrap();

    {
        let mut events = client.poll(DT);
        assert_matches!(
            events.next().unwrap(),
            ClientEvent::Recv { msg, lane }
            if msg == S2C && lane == LANE
        );
        assert!(events.next().is_none());
    }
}

#[test]
fn client_disconnect() {
    let (mut client, mut server, target_key) = open();

    client.disconnect(REASON).unwrap();

    {
        let mut events = client.poll(DT);
        assert_matches!(
            events.next().unwrap(),
            ClientEvent::Disconnected { reason: DisconnectReason::Local(reason) }
            if reason == REASON
        );
        assert!(events.next().is_none());
    }

    {
        let mut events = server.poll(DT);
        assert_matches!(
            events.next().unwrap(),
            ServerEvent::Disconnected { client_key, reason: DisconnectReason::Remote(reason), .. }
            if client_key == target_key && reason == REASON
        );
        assert!(events.next().is_none());
    }
}

#[test]
fn server_disconnect() {
    let (mut client, mut server, target_key) = open();

    server.disconnect(target_key, REASON).unwrap();

    {
        let mut events = client.poll(DT);
        assert_matches!(
            events.next().unwrap(),
            ClientEvent::Disconnected { reason: DisconnectReason::Remote(reason) }
            if reason == REASON
        );
        assert!(events.next().is_none());
    }

    {
        let mut events = server.poll(DT);
        assert_matches!(
            events.next().unwrap(),
            ServerEvent::Disconnected { client_key, reason: DisconnectReason::Local(reason) }
            if client_key == target_key && reason == REASON
        );
        assert!(events.next().is_none());
    }
}

#[test]
fn server_close() {
    let (mut client, mut server, _) = open();

    server.close(REASON).unwrap();

    {
        let mut events = client.poll(DT);
        assert_matches!(
            events.next().unwrap(),
            ClientEvent::Disconnected { reason: DisconnectReason::Remote(reason) }
            if reason == REASON
        );
        assert!(events.next().is_none());
    }
    {
        let mut events = server.poll(DT);
        assert_matches!(
            events.next().unwrap(),
            ServerEvent::Closed { reason: CloseReason::Local(reason) }
            if reason == REASON
        );
    }
}

#[test]
fn server_drop() {
    let (mut client, server, _) = open();

    drop(server);
    {
        let mut events = client.poll(DT);
        assert_matches!(
            events.next().unwrap(),
            ClientEvent::Disconnected { reason: DisconnectReason::Remote(reason) }
            if reason == DROP_DISCONNECT_REASON
        );
        assert!(events.next().is_none());
    }
}
