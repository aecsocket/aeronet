use bytes::Bytes;
use futures::{channel::mpsc, never::Never, SinkExt, StreamExt};
use tracing::debug;
use web_time::Duration;
use xwt_core::session::datagram;

pub const MSG_BUF_CAP: usize = 256;

// conservative estimate based on
// https://blog.cloudflare.com/increasing-ipv6-mtu/
pub const MIN_MTU: usize = 1024;

const STATS_UPDATE_INTERVAL: Duration = Duration::from_millis(500);

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        pub type Connection = xwt_web_sys::Session;
        pub type ClientEndpoint = xwt_web_sys::Endpoint;

        pub fn get_mtu(_: &Connection) -> Option<usize> {
            // TODO this MUST be done
            None
        }

        pub fn get_rtt(_: &Connection) -> Duration {
            // unsupported in most browsers
            // https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/getStats
            Duration::ZERO
        }

        pub fn to_bytes(datagram: Datagram) -> Bytes {
            Bytes::from(datagram)
        }

        async fn send_datagram(conn: &Connection, msg: Bytes) -> Result<(), Error> {
            datagram::Send::send_datagram(conn, msg).await;
        }
    } else {
        pub type Connection = xwt_wtransport::Connection;
        pub type ClientEndpoint = xwt_wtransport::Endpoint<xwt_wtransport::wtransport::endpoint::endpoint_side::Client>;

        pub fn get_mtu(conn: &Connection) -> Option<usize> {
            conn.0.max_datagram_size()
        }

        pub fn get_rtt(conn: &Connection) -> Duration {
            conn.0.rtt()
        }

        pub fn to_bytes(datagram: Datagram) -> Bytes {
            datagram.0.payload()
        }

        async fn send_datagram(conn: &Connection, msg: Bytes) -> Result<(), Error> {
            use wtransport::error::SendDatagramError;

            let msg_len = msg.len();
            match datagram::Send::send_datagram(conn, msg).await {
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
                    let mtu = get_mtu(conn);
                    debug!("Attempted to send datagram of size {msg_len} when connection only supports {mtu:?}");
                    Ok(())
                }
                Err(SendDatagramError::UnsupportedByPeer) => {
                    // this should be impossible, since we checked that the client does support datagrams
                    // before connecting, but we'll error-case it anyway
                    return Err(Error::DatagramsNotSupported);
                }
            }
        }
    }
}

pub type Datagram = <Connection as datagram::Receive>::Datagram;

#[derive(Debug)]
pub struct ConnectionMeta {
    pub rtt: Duration,
    pub mtu: usize,
}

// intentionally don't derive Error so that consumers are forced to map each
// variant to their own error variant
#[derive(Debug)]
pub enum Error {
    FrontendClosed,
    ConnectionLost(<Connection as datagram::Receive>::Error),
    DatagramsNotSupported,
}

pub async fn send_loop(
    conn: &Connection,
    mut recv_s: mpsc::UnboundedReceiver<Bytes>,
) -> Result<Never, Error> {
    loop {
        let msg = recv_s.next().await.ok_or(Error::FrontendClosed)?;
        send_datagram(conn, msg).await?;
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

pub async fn update_meta(
    conn: &Connection,
    mut send_meta: mpsc::Sender<ConnectionMeta>,
) -> Result<Never, Error> {
    loop {
        sleep(STATS_UPDATE_INTERVAL).await;
        let meta = ConnectionMeta {
            rtt: get_rtt(conn),
            mtu: get_mtu(conn).ok_or(Error::DatagramsNotSupported)?,
        };
        send_meta
            .send(meta)
            .await
            .map_err(|_| Error::FrontendClosed)?;
    }
}

#[cfg(target_family = "wasm")]
async fn sleep(duration: Duration) {
    gloo_timers::future::sleep(duration).await
}

#[cfg(not(target_family = "wasm"))]
async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
}
