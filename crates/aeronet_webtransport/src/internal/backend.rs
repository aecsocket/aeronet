use bytes::Bytes;
use futures::{channel::mpsc, never::Never, FutureExt, SinkExt, StreamExt};
use web_time::Duration;
use xwt_core::session::datagram;

use crate::runtime::WebTransportRuntime;

use super::{get_mtu, Connection, ConnectionMeta, InternalError};

const STATS_UPDATE_INTERVAL: Duration = Duration::from_millis(500);

pub async fn handle_connection<E>(
    runtime: WebTransportRuntime,
    conn: Connection,
    mut recv_s: mpsc::UnboundedReceiver<Bytes>,
    mut send_r: mpsc::Sender<Bytes>,
    mut send_meta: mpsc::Sender<ConnectionMeta>,
) -> Result<Never, InternalError<E>> {
    #[cfg(target_family = "wasm")]
    {
        todo!("connections on WASM are not supported yet")
    }

    #[cfg(not(target_family = "wasm"))]
    {
        use wtransport::error::SendDatagramError;

        let conn = &conn;
        let send = async move {
            loop {
                let packet = recv_s
                    .next()
                    .await
                    .ok_or(InternalError::<E>::FrontendClosed)?;
                let packet_len = packet.len();
                match datagram::Send::send_datagram(conn, packet).await {
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
                        Err(InternalError::<E>::DatagramsNotSupported)
                    }
                }?;
            }
        };

        let recv = async move {
            loop {
                let packet = datagram::Receive::receive_datagram(conn)
                    .await
                    .map_err(|err| InternalError::ConnectionLost(err.into()))?;
                let packet = packet.0.payload();
                send_r
                    .send(packet)
                    .await
                    .map_err(|_| InternalError::FrontendClosed)?;
            }
        };

        let meta = async move {
            loop {
                runtime.sleep(STATS_UPDATE_INTERVAL).await;
                let meta = ConnectionMeta {
                    rtt: conn.0.rtt(),
                    mtu: get_mtu(conn).ok_or(InternalError::DatagramsNotSupported)?,
                };
                send_meta
                    .send(meta)
                    .await
                    .map_err(|_| InternalError::FrontendClosed)?;
            }
        };

        futures::select! {
            r = send.fuse() => r,
            r = recv.fuse() => r,
            r = meta.fuse() => r,
        }
    }
}
