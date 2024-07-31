use std::sync::Arc;

use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    FutureExt, SinkExt, StreamExt,
};
use web_time::Duration;
use xwt_core::{
    session::datagram::{Receive, Send},
    utils::maybe,
};

use crate::{
    client::{ClientConfig, ClientError},
    runtime::WebTransportRuntime,
};

use super::{get_mtu, ClientEndpoint, Connection, ConnectionMeta, InternalError};

#[allow(dead_code)]
const STATS_UPDATE_INTERVAL: Duration = Duration::from_millis(500);

#[allow(clippy::unnecessary_wraps)] // on WASM, must match fn sig
pub fn create_client_endpoint(config: ClientConfig) -> Result<ClientEndpoint, ClientError> {
    #[cfg(target_family = "wasm")]
    {
        Ok(xwt_web_sys::Endpoint {
            options: config.to_js(),
        })
    }

    #[cfg(not(target_family = "wasm"))]
    {
        let raw = wtransport::Endpoint::client(config).map_err(ClientError::CreateEndpoint)?;
        Ok(xwt_wtransport::Endpoint(raw))
    }
}

pub async fn handle_connection<E: maybe::Send + 'static>(
    runtime: WebTransportRuntime,
    conn: Connection,
    recv_s: mpsc::UnboundedReceiver<Bytes>,
    send_r: mpsc::Sender<Bytes>,
    send_meta: mpsc::Sender<ConnectionMeta>,
) -> Result<Never, InternalError<E>> {
    let conn = Arc::new(conn);
    let (send_err, mut recv_err) = mpsc::channel::<InternalError<E>>(1);

    let (_send_sending_closed, recv_sending_closed) = oneshot::channel();
    runtime.spawn({
        let conn = conn.clone();
        let mut send_err = send_err.clone();
        async move {
            let err = send_loop(conn, recv_sending_closed, recv_s)
                .await
                .unwrap_err();
            let _ = send_err.send(err).await;
        }
    });

    let (_send_receiving_closed, recv_receiving_closed) = oneshot::channel();
    runtime.spawn({
        let conn = conn.clone();
        let mut send_err = send_err.clone();
        async move {
            let err = recv_loop(conn, recv_receiving_closed, send_r)
                .await
                .unwrap_err();
            let _ = send_err.send(err).await;
        }
    });

    let (_send_meta_closed, recv_meta_closed) = oneshot::channel();
    runtime.spawn({
        let runtime = runtime.clone();
        let conn = conn.clone();
        let mut send_err = send_err.clone();
        async move {
            let err = meta_loop(runtime, conn, recv_meta_closed, send_meta)
                .await
                .unwrap_err();
            let _ = send_err.send(err).await;
        }
    });

    let err = recv_err
        .next()
        .await
        .unwrap_or(InternalError::BackendClosed);
    Err(err)
}

async fn send_loop<E>(
    conn: Arc<Connection>,
    mut recv_closed: oneshot::Receiver<()>,
    mut recv_s: mpsc::UnboundedReceiver<Bytes>,
) -> Result<(), InternalError<E>> {
    loop {
        let packet = futures::select! {
            x = recv_s.next() => x,
            _ = recv_closed => return Ok(()),
        }
        .ok_or(InternalError::FrontendClosed)?;

        #[cfg(target_family = "wasm")]
        {
            conn.send_datagram(packet)
                .await
                .map_err(|err| InternalError::ConnectionLost(err.into()))?;
        }

        #[cfg(not(target_family = "wasm"))]
        {
            use wtransport::error::SendDatagramError;

            let packet_len = packet.len();
            match conn.send_datagram(packet).await {
                Ok(()) => Ok(()),
                Err(SendDatagramError::NotConnected) => {
                    // we'll pick up connection errors in the recv loop,
                    // where we'll get a better error message
                    Ok(())
                }
                Err(SendDatagramError::TooLarge) => {
                    // the backend constantly informs the frontend about changes in the path MTU
                    // so hopefully the frontend will realise its packets are exceeding MTU,
                    // and shrink them accordingly; therefore this is just a one-off error
                    let mtu = get_mtu(&conn);
                    tracing::debug!(
                        packet_len,
                        mtu,
                        "Attempted to send datagram larger than MTU"
                    );
                    Ok(())
                }
                Err(SendDatagramError::UnsupportedByPeer) => {
                    // this should be impossible, since we checked that the client does support datagrams
                    // before connecting, but we'll error-case it anyway
                    Err(InternalError::DatagramsNotSupported)
                }
            }?;
        }
    }
}

async fn recv_loop<E>(
    conn: Arc<Connection>,
    mut recv_closed: oneshot::Receiver<()>,
    mut send_r: mpsc::Sender<Bytes>,
) -> Result<(), InternalError<E>> {
    loop {
        #[allow(clippy::useless_conversion)] // multi-target support
        let packet = futures::select! {
            x = conn.receive_datagram().fuse() => x,
            _ = recv_closed => return Ok(()),
        }
        .map_err(|err| InternalError::ConnectionLost(err.into()))?;

        let packet = {
            #[cfg(target_family = "wasm")]
            {
                Bytes::from(packet)
            }

            #[cfg(not(target_family = "wasm"))]
            {
                packet.0.payload()
            }
        };
        send_r
            .send(packet)
            .await
            .map_err(|_| InternalError::FrontendClosed)?;
    }
}

async fn meta_loop<E>(
    runtime: WebTransportRuntime,
    conn: Arc<Connection>,
    mut recv_closed: oneshot::Receiver<()>,
    mut send_meta: mpsc::Sender<ConnectionMeta>,
) -> Result<(), InternalError<E>> {
    loop {
        futures::select! {
            _ = runtime.sleep(STATS_UPDATE_INTERVAL).fuse() => {},
            _ = recv_closed => return Ok(()),
        };

        let meta = ConnectionMeta {
            #[cfg(not(target_family = "wasm"))]
            rtt: conn.0.rtt(),
            mtu: get_mtu(&conn).ok_or(InternalError::DatagramsNotSupported)?,
        };
        send_meta
            .send(meta)
            .await
            .map_err(|_| InternalError::FrontendClosed)?;
    }
}
