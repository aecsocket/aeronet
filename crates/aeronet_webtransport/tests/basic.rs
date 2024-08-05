use std::{
    net::{Ipv4Addr, SocketAddr},
    process::Output,
    time::Duration,
};

use aeronet::{
    client::{ClientEvent, ClientTransport},
    lane::LaneKind,
    server::{ServerEvent, ServerTransport},
};
use aeronet_proto::session::SessionConfig;
use aeronet_webtransport::{
    client::{ClientConfig, WebTransportClient},
    runtime::WebTransportRuntime,
    server::{ServerConfig, WebTransportServer},
};
use assert_matches::assert_matches;
use web_time::Instant;

const TIMEOUT: Duration = Duration::from_millis(500);
const DT: Duration = Duration::ZERO;

#[test]
fn connect() {
    let runtime = WebTransportRuntime::default();
    let port = rand::random::<u16>();
    let session_config = SessionConfig::default().with_lanes([LaneKind::ReliableOrdered]);

    let mut server = WebTransportServer::new();
    let identity = wtransport::Identity::self_signed(["127.0.0.1"]).unwrap();
    let server_config = ServerConfig::builder()
        .with_bind_address(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port))
        .with_identity(&identity)
        .max_idle_timeout(Some(TIMEOUT))
        .unwrap()
        .build();
    server
        .open(&runtime, server_config, session_config.clone())
        .unwrap();

    let mut client = WebTransportClient::new();
    let client_config = ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .max_idle_timeout(Some(TIMEOUT))
        .unwrap()
        .build();
    client
        .connect(
            &runtime,
            client_config,
            session_config,
            format!("https://127.0.0.1:{port}"),
        )
        .unwrap();

    {
        let mut events = server.poll_blocking();
        assert_matches!(events.next().unwrap(), ServerEvent::Opened);
        assert!(events.next().is_none());
    }

    assert_matches!(
        client.poll_blocking().next().unwrap(),
        ClientEvent::Connected
    );
}

trait PollBlocking {
    type Output;

    fn poll_blocking(&mut self) -> impl Iterator<Item = Self::Output>;
}

impl PollBlocking for WebTransportClient {
    type Output = ClientEvent<Self>;

    fn poll_blocking(&mut self) -> impl Iterator<Item = Self::Output> {
        let start = Instant::now();
        while Instant::now().duration_since(start) < TIMEOUT {
            let mut events = self.poll(DT).peekable();
            if events.peek().is_some() {
                return events.collect::<Vec<_>>().into_iter();
            }
        }
        panic!("timed out");
    }
}

impl PollBlocking for WebTransportServer {
    type Output = ServerEvent<Self>;

    fn poll_blocking(&mut self) -> impl Iterator<Item = Self::Output> {
        let start = Instant::now();
        while Instant::now().duration_since(start) < TIMEOUT {
            let mut events = self.poll(DT).peekable();
            if events.peek().is_some() {
                return events.collect::<Vec<_>>().into_iter();
            }
        }
        panic!("timed out");
    }
}
