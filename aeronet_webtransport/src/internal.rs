use bytes::Bytes;
use futures::{channel::mpsc, never::Never, SinkExt, StreamExt};
use web_time::Duration;
use xwt_core::session::datagram;

pub const MSG_BUF_CAP: usize = 256;
const STATS_UPDATE_INTERVAL: Duration = Duration::from_millis(500);

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        pub type Connection = xwt_web_sys::Session;
        pub type ClientEndpoint = xwt_web_sys::Endpoint;

        pub fn check_datagram_support(_: &Connection) -> bool {
            // TODO I think there's a way to do this on wasm
            true
        }

        pub fn rtt_of(_: &Connection) -> Duration {
            // unsupported in most browsers
            // https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/getStats
            Duration::ZERO
        }

        pub fn to_bytes(datagram: Datagram) -> Bytes {
            Bytes::from(datagram)
        }
    } else {
        pub type Connection = xwt_wtransport::Connection;
        pub type ClientEndpoint = xwt_wtransport::Endpoint<xwt_wtransport::wtransport::endpoint::endpoint_side::Client>;

        pub fn supports_datagrams(conn: &Connection) -> bool {
            conn.0.max_datagram_size().is_some()
        }

        pub fn rtt_of(conn: &Connection) -> Duration {
            conn.0.rtt()
        }

        pub fn to_bytes(datagram: Datagram) -> Bytes {
            datagram.0.payload()
        }
    }
}

pub type Datagram = <Connection as datagram::Receive>::Datagram;

// intentionally don't derive Error so that consumers are forced to map each
// variant to their own error variant
#[derive(Debug)]
pub enum Error {
    FrontendClosed,
    ConnectionLost(<Connection as datagram::Receive>::Error),
    SendDatagram(<Connection as datagram::Send>::Error),
}

pub async fn send_loop(
    conn: &Connection,
    mut recv_s: mpsc::UnboundedReceiver<Bytes>,
) -> Result<Never, Error> {
    loop {
        let msg = recv_s.next().await.ok_or(Error::FrontendClosed)?;
        datagram::Send::send_datagram(conn, msg)
            .await
            .map_err(Error::SendDatagram)?;
    }
}

pub async fn recv_loop(conn: &Connection, mut send_r: mpsc::Sender<Bytes>) -> Result<Never, Error> {
    loop {
        let msg = datagram::Receive::receive_datagram(conn)
            .await
            .map_err(Error::ConnectionLost)?;
        send_r
            .send(to_bytes(msg))
            .await
            .map_err(|_| Error::FrontendClosed)?;
    }
}

pub async fn update_rtt_loop(
    conn: &Connection,
    mut send_rtt: mpsc::Sender<Duration>,
) -> Result<Never, Error> {
    loop {
        tokio::time::sleep(STATS_UPDATE_INTERVAL).await;
        send_rtt
            .send(rtt_of(conn))
            .await
            .map_err(|_| Error::FrontendClosed)?;
    }
}
