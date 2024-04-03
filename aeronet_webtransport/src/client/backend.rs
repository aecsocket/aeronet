use aeronet::protocol::ProtocolVersion;
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    FutureExt,
};
use tracing::debug;
use xwt_core::{Connecting, EndpointConnect, OpenBiStream, OpeningBiStream};

use crate::{
    internal,
    shared::{self, ConnectionStats},
    ty,
};

use super::{ClientBackendError, NativeConfig};

#[derive(Debug)]
pub struct Connected {
    pub send_c2s: mpsc::UnboundedSender<Bytes>,
    pub recv_s2c: mpsc::Receiver<Bytes>,
    pub recv_stats: mpsc::Receiver<ConnectionStats>,
    pub initial_stats: ConnectionStats,
}

pub async fn start(
    native_config: NativeConfig,
    version: ProtocolVersion,
    target: String,
    send_connected: oneshot::Sender<Connected>,
) -> Result<Never, ClientBackendError> {
    let endpoint = create_endpoint(native_config)?;
    debug!("Connecting to {target:?}");
    let conn = endpoint
        .connect(&target)
        .await
        .map_err(|err| ClientBackendError::StartConnecting(err.into()))?
        .wait_connect()
        .await
        .map_err(|err| ClientBackendError::AwaitConnection(err.into()))?;

    if !internal::check_datagram_support(&conn) {
        Err(shared::BackendError::DatagramsNotSupported)?;
    }

    debug!("Connection opened, opening managed stream");
    let (mut send_managed, mut recv_managed) = conn
        .open_bi()
        .await
        .map_err(|err| shared::BackendError::StartOpeningManaged(err.into()))?
        .wait_bi()
        .await
        .map_err(|err| shared::BackendError::AwaitOpeningManaged(err.into()))?;

    debug!("Managed stream open, negotiating protocol");
    internal::negotiate::client(version, &mut send_managed, &mut recv_managed).await?;

    debug!("Negotiated successfully, forwarding to frontend");
    let (send_c2s, recv_c2s) = mpsc::unbounded::<Bytes>();
    let (send_s2c, recv_s2c) = mpsc::channel::<Bytes>(internal::BUFFER_SIZE);
    let (send_stats, recv_stats) = mpsc::channel::<ConnectionStats>(1);
    send_connected
        .send(Connected {
            send_c2s,
            recv_s2c,
            recv_stats,
            initial_stats: ConnectionStats::from(&conn),
        })
        .map_err(|_| shared::BackendError::FrontendClosed)?;

    debug!("Starting connection loop");
    // `receive_datagram` may not be cancel-safe, so we create two futures which
    // loop infinitely independently, and wait for the first one to fail
    let send = internal::send(&conn, recv_c2s);
    let recv = internal::recv(&conn, send_s2c, send_stats);
    futures::select! {
        r = send.fuse() => r,
        r = recv.fuse() => r,
    }
    .map_err(From::from)
}

#[cfg(target_family = "wasm")]
fn create_endpoint(
    config: web_sys::WebTransportOptions,
) -> Result<ty::Endpoint, ClientBackendError> {
    Ok(ty::Endpoint { options: config })
}

#[cfg(not(target_family = "wasm"))]
fn create_endpoint(config: wtransport::ClientConfig) -> Result<ty::Endpoint, ClientBackendError> {
    let endpoint =
        wtransport::Endpoint::client(config).map_err(shared::BackendError::CreateEndpoint)?;
    Ok(xwt::current::Endpoint(endpoint))
}
