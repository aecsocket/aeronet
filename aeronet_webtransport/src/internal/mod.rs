pub mod negotiate;

use bytes::Bytes;
use futures::{channel::mpsc, never::Never, SinkExt, StreamExt};
use xwt_core::datagram::{Receive, Send};

use crate::{error::BackendError, transport::ConnectionStats, ty};

pub const BUFFER_SIZE: usize = 32;

#[cfg(target_family = "wasm")]
pub fn check_datagram_support(_: &ty::Connection) -> bool {
    true // TODO I think there's a way to do this on wasm
}

#[cfg(not(target_family = "wasm"))]
pub fn check_datagram_support(conn: &ty::Connection) -> bool {
    conn.0.max_datagram_size().is_some()
}

pub async fn send(
    conn: &ty::Connection,
    mut recv_s: mpsc::UnboundedReceiver<Bytes>,
) -> Result<Never, BackendError> {
    loop {
        let packet = recv_s.next().await.ok_or(BackendError::FrontendClosed)?;
        conn.send_datagram(packet)
            .await
            .map_err(|err| BackendError::SendDatagram(err.into()))?;
    }
}

pub async fn recv(
    conn: &ty::Connection,
    mut send_r: mpsc::Sender<Bytes>,
    mut send_stats: mpsc::Sender<ConnectionStats>,
) -> Result<Never, BackendError> {
    loop {
        let stats = ConnectionStats::from(conn);
        if let Err(err) = send_stats.try_send(stats) {
            if err.is_disconnected() {
                Err(BackendError::FrontendClosed)?;
            }
        }

        let packet = conn
            .receive_datagram()
            .await
            .map_err(|err| BackendError::RecvDatagram(err.into()))?;
        send_r
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
