use aeronet::protocol::ProtocolVersion;
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    FutureExt, SinkExt, StreamExt,
};
use tracing::debug;
use xwt_core::{
    datagram::{Receive, Send},
    Connecting, EndpointConnect, OpenBiStream, OpeningBiStream,
};

use crate::{error::BackendError, internal, transport::ConnectionStats, ty::*};

use super::NativeConfig;

const BUFFER_SIZE: usize = 32;

#[derive(Debug)]
pub struct Connected {
    pub recv_stats: mpsc::Receiver<ConnectionStats>,
    pub send_c2s: mpsc::UnboundedSender<Bytes>,
    pub recv_s2c: mpsc::Receiver<Bytes>,
}

pub async fn open(
    config: NativeConfig,
    url: String,
    version: ProtocolVersion,
    send_connected: oneshot::Sender<Connected>,
) -> Result<Never, BackendError> {
    let endpoint = create_endpoint(config)?;
    debug!("Connecting to {url:?}");
    let conn = endpoint
        .connect(&url)
        .await
        .map_err(|err| BackendError::StartConnecting(err.into()))?
        .wait_connect()
        .await
        .map_err(|err| BackendError::AwaitConnection(err.into()))?;

    debug!("Connection opened, opening managed stream");
    let (mut send_managed, mut recv_managed) = conn
        .open_bi()
        .await
        .map_err(|err| BackendError::StartOpeningManaged(err.into()))?
        .wait_bi()
        .await
        .map_err(|err| BackendError::AwaitOpeningManaged(err.into()))?;

    debug!("Managed stream open, negotiating protocol");
    internal::negotiate::client(version, &mut send_managed, &mut recv_managed).await?;

    debug!("Connection established, forwarding channels to frontend");
    let (send_stats, recv_stats) = mpsc::channel::<ConnectionStats>(1);
    let (send_c2s, recv_c2s) = mpsc::unbounded::<Bytes>();
    let (send_s2c, recv_s2c) = mpsc::channel::<Bytes>(BUFFER_SIZE);
    send_connected
        .send(Connected {
            recv_stats,
            send_c2s,
            recv_s2c,
        })
        .map_err(|_| BackendError::FrontendClosed)?;

    debug!("Starting connection loop");
    let send = send(&conn, recv_c2s);
    let recv = recv(&conn, send_s2c, send_stats);
    futures::select! {
        r = send.fuse() => r,
        r = recv.fuse() => r,
    }
}

#[cfg(target_family = "wasm")]
fn create_endpoint(config: web_sys::WebTransportOptions) -> Result<ty::Endpoint, BackendError> {
    Ok(xwt::current::Endpoint { options: config })
}

#[cfg(not(target_family = "wasm"))]
fn create_endpoint(config: wtransport::ClientConfig) -> Result<ty::Endpoint, BackendError> {
    let endpoint = wtransport::Endpoint::client(config).map_err(BackendError::CreateEndpoint)?;
    Ok(xwt::current::Endpoint(endpoint))
}

async fn send(
    conn: &ty::Connection,
    mut recv_c2s: mpsc::UnboundedReceiver<Bytes>,
) -> Result<Never, BackendError> {
    loop {
        let packet = recv_c2s.next().await.ok_or(BackendError::FrontendClosed)?;
        conn.send_datagram(packet)
            .await
            .map_err(|err| BackendError::SendDatagram(err.into()))?;
    }
}

async fn recv(
    conn: &ty::Connection,
    mut send_s2c: mpsc::Sender<Bytes>,
    mut send_stats: mpsc::Sender<ConnectionStats>,
) -> Result<Never, BackendError> {
    loop {
        let stats = ConnectionStats::from(conn);
        if let Err(err) = send_stats.try_send(stats) {
            if err.is_disconnected() {
                return Err(BackendError::FrontendClosed);
            }
        }

        let packet = conn
            .receive_datagram()
            .await
            .map_err(|err| BackendError::RecvDatagram(err.into()))?;
        send_s2c
            .send(to_bytes(packet))
            .await
            .map_err(|_| BackendError::FrontendClosed)?;
    }
}

// optimization: avoid as much reallocation as possible
// * wtransport: use the `wtransport::Datagram::payload() -> Bytes`
// * web-sys: use the `Vec<u8>` directly
// TODO upstream this to xwt

#[cfg(target_family = "wasm")]
fn to_bytes(datagram: ty::Datagram) -> Bytes {
    debug_assert_eq!(datagram.capacity(), datagram.len());
    Bytes::from(datagram)
}

#[cfg(not(target_family = "wasm"))]
fn to_bytes(datagram: ty::Datagram) -> Bytes {
    datagram.0.payload()
}
