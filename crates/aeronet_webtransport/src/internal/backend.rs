use bytes::Bytes;
use futures::{channel::mpsc, never::Never, SinkExt, StreamExt};
use web_time::Duration;
use xwt_core::session::datagram;

use super::{get_mtu, send_datagram, to_bytes, Connection, ConnectionMeta, InternalError};

const STATS_UPDATE_INTERVAL: Duration = Duration::from_millis(500);

pub async fn send_loop<E>(
    conn: &Connection,
    mut recv_s: mpsc::UnboundedReceiver<Bytes>,
) -> Result<Never, InternalError<E>> {
    loop {
        let msg = recv_s.next().await.ok_or(InternalError::FrontendClosed)?;
        send_datagram(conn, msg).await?;
    }
}

pub async fn recv_loop<E>(
    conn: &Connection,
    mut send_r: mpsc::Sender<Bytes>,
) -> Result<Never, InternalError<E>> {
    loop {
        #[allow(clippy::useless_conversion)] // WASM needs the .into()
        let msg = datagram::Receive::receive_datagram(conn)
            .await
            .map_err(|err| InternalError::ConnectionLost(err.into()))?;
        let msg = to_bytes(msg);
        send_r
            .send(msg)
            .await
            .map_err(|_| InternalError::FrontendClosed)?;
    }
}

pub async fn update_meta<E>(
    conn: &Connection,
    mut send_meta: mpsc::Sender<ConnectionMeta>,
) -> Result<Never, InternalError<E>> {
    loop {
        sleep(STATS_UPDATE_INTERVAL).await;
        let meta = ConnectionMeta {
            #[cfg(not(target_family = "wasm"))]
            rtt: conn.0.rtt(),
            mtu: get_mtu(conn).ok_or(InternalError::DatagramsNotSupported)?,
        };
        send_meta
            .send(meta)
            .await
            .map_err(|_| InternalError::FrontendClosed)?;
    }
}

#[cfg(target_family = "wasm")]
async fn sleep(duration: Duration) {
    gloo_timers::future::sleep(duration).await;
}

#[cfg(not(target_family = "wasm"))]
async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
}
