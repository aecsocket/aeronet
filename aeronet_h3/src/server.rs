use std::{io, marker::PhantomData, net::SocketAddr};

use aeronet::{ServerTransport, ServerTransportError, ServerTransportEvent, TransportSettings};
use bytes::Bytes;
use h3::{ext::Protocol, server::Connection};
pub use h3_quinn::quinn;
use h3_webtransport::server::WebTransportSession;
use http::Method;
use quinn::{Connecting, Endpoint, ServerConfig};
use tokio::sync::mpsc;

use crate::BUFFER_SIZE;

type EventReceiver = mpsc::Receiver<Result<ServerTransportEvent, anyhow::Error>>;
type EventSender = mpsc::Sender<Result<ServerTransportEvent, anyhow::Error>>;

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct H3ServerTransport<S: TransportSettings> {
    events: EventReceiver,
    p: PhantomData<S>,
}

impl<S: TransportSettings> H3ServerTransport<S> {
    pub async fn new(config: ServerConfig, addr: SocketAddr) -> io::Result<Self> {
        let endpoint = Endpoint::server(config, addr)?;
        let (send_errors, recv_errors) =
            mpsc::channel::<Result<ServerTransportEvent, anyhow::Error>>(BUFFER_SIZE);

        tokio::spawn(async move {
            update_endpoint(endpoint).await;
        });

        Ok(H3ServerTransport {
            events: recv_errors,
            p: PhantomData::default(),
        })
    }
}

impl<S: TransportSettings> ServerTransport<S> for H3ServerTransport<S> {
    fn pop_event(&mut self) -> Option<ServerTransportEvent> {
        None
    }

    fn recv(&mut self, from: aeronet::ClientId) -> Result<Option<S::C2S>, anyhow::Error> {
        Ok(None)
    }

    fn send(&mut self, to: aeronet::ClientId, msg: impl Into<S::S2C>) -> Result<(), anyhow::Error> {
        Ok(())
    }

    fn disconnect(&mut self, client: aeronet::ClientId) -> anyhow::Result<()> {
        Ok(())
    }
}

async fn update_endpoint(endpoint: Endpoint) {
    while let Some(conn) = endpoint.accept().await {
        tokio::spawn(async move {
            match conn.await {
                Ok(conn) => {
                    accept_connection(conn).await;
                }
                Err(err) => {
                    // TODO
                }
            }
        });
    }

    endpoint.wait_idle().await;
}

async fn accept_connection(conn: quinn::Connection) -> Result<(), h3::Error> {
    let h3_conn = h3::server::builder()
        .enable_webtransport(true)
        .enable_connect(true)
        .enable_datagram(true)
        .max_webtransport_sessions(1)
        .send_grease(true)
        .build(h3_quinn::Connection::new(conn))
        .await?;

    handle_connection(h3_conn).await
}

async fn handle_connection(
    mut conn: Connection<h3_quinn::Connection, Bytes>,
) -> Result<(), h3::Error> {
    loop {
        match conn.accept().await? {
            Some((req, stream)) => {
                let ext = req.extensions();
                match req.method() {
                    &Method::CONNECT if ext.get::<Protocol>() == Some(&Protocol::WEB_TRANSPORT) => {
                        let session = WebTransportSession::accept(req, stream, conn).await?;
                        return Ok(());
                    }
                    _ => {
                        // TODO
                    }
                }
            }
            None => break,
        }
    }
    Ok(())
}
