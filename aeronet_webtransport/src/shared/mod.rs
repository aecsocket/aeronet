mod negotiate;

use derivative::Derivative;
use tracing::debug;
use xwt::current::{Connection, RecvStream, SendStream};
use xwt_core::datagram::{Receive, Send};

use std::time::Duration;

use aeronet::{LaneConfig, LaneKey, OnLane, ProtocolVersion, TryAsBytes, TryFromBytes};
use aeronet_protocol::Lanes;
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    FutureExt, SinkExt, StreamExt,
};

use crate::{BackendError, ConnectionInfo, WebTransportError};

const MSG_BUF_CAP: usize = 64;

#[derive(Debug)]
pub struct ConnectionFrontend {
    pub info: ConnectionInfo,
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_s2c: mpsc::Receiver<Bytes>,
    recv_rtt: mpsc::Receiver<Duration>,
    recv_err: oneshot::Receiver<BackendError>,
    lanes: Lanes,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct ConnectionBackend {
    recv_c2s: mpsc::UnboundedReceiver<Bytes>,
    send_s2c: mpsc::Sender<Bytes>,
    send_rtt: mpsc::Sender<Duration>,
    send_err: oneshot::Sender<BackendError>,
    #[derivative(Debug = "ignore")]
    _send_managed: SendStream,
    #[derivative(Debug = "ignore")]
    _recv_managed: RecvStream,
}

pub async fn connection_channel<const SERVER: bool>(
    conn: &Connection,
    version: ProtocolVersion,
    max_packet_len: usize,
    lanes: &[LaneConfig],
) -> Result<(ConnectionFrontend, ConnectionBackend), BackendError> {
    let (send_managed, recv_managed) = if SERVER {
        negotiate::server(conn, version).await?
    } else {
        negotiate::client(conn, version).await?
    };

    let (send_c2s, recv_c2s) = mpsc::unbounded();
    let (send_s2c, recv_s2c) = mpsc::channel(MSG_BUF_CAP);
    let (send_rtt, recv_rtt) = mpsc::channel(1);
    let (send_err, recv_err) = oneshot::channel();
    Ok((
        ConnectionFrontend {
            info: ConnectionInfo::from(conn),
            send_c2s,
            recv_s2c,
            recv_rtt,
            recv_err,
            lanes: Lanes::new(max_packet_len, lanes),
        },
        ConnectionBackend {
            recv_c2s,
            send_s2c,
            send_rtt,
            send_err,
            // we have to keep the managed streams alive
            // so we'll just pass them to the backend
            // this also lets us expand the functionality of managed streams
            // in the future
            _send_managed: send_managed,
            _recv_managed: recv_managed,
        },
    ))
}

impl ConnectionFrontend {
    pub fn update(&mut self) {
        while let Ok(Some(rtt)) = self.recv_rtt.try_next() {
            self.info.rtt = rtt;
        }
    }

    pub fn send<S, R>(&mut self, msg: S) -> Result<(), WebTransportError<S, R>>
    where
        S: TryAsBytes + OnLane,
        R: TryFromBytes,
    {
        let msg_bytes = msg.try_as_bytes().map_err(WebTransportError::AsBytes)?;
        let msg_bytes = msg_bytes.as_ref();

        for packet in self
            .lanes
            .send(msg_bytes, msg.lane().index())
            .map_err(BackendError::LaneSend)?
        {
            let mut bytes = vec![0; packet.header.len() + packet.payload.len()].into_boxed_slice();
            bytes[..packet.header.len()].copy_from_slice(&packet.header);
            bytes[packet.header.len()..].copy_from_slice(packet.payload);

            self.info.total_bytes_sent += bytes.len();
            self.send_c2s
                .unbounded_send(Bytes::from(bytes))
                .map_err(|_| BackendError::Closed)?;
        }
        self.info.msg_bytes_sent += msg_bytes.len();
        self.info.msgs_sent += 1;
        Ok(())
    }

    pub fn recv<S, R>(&mut self) -> Result<Option<R>, WebTransportError<S, R>>
    where
        S: TryAsBytes + OnLane,
        R: TryFromBytes,
    {
        while let Ok(Some(packet)) = self.recv_s2c.try_next() {
            self.info.total_bytes_recv += packet.len();
            if let Some(msg_bytes) = self
                .lanes
                .recv(&packet)
                .map_err(|err| WebTransportError::Backend(BackendError::LaneRecv(err)))?
            {
                let msg = R::try_from_bytes(&msg_bytes).map_err(WebTransportError::FromBytes)?;
                self.info.msg_bytes_recv += msg_bytes.len();
                self.info.msgs_recv += 1;
                return Ok(Some(msg));
            }
        }
        Ok(None)
    }

    pub fn recv_err(&mut self) -> Result<(), BackendError> {
        match self.recv_err.try_recv() {
            Ok(None) => Ok(()),
            Ok(Some(err)) => Err(err),
            Err(_) => Err(BackendError::Closed),
        }
    }
}

impl ConnectionBackend {
    pub async fn handle(self, conn: Connection) {
        debug!("Connected backend");
        match try_handle_connection(
            conn,
            self.recv_c2s,
            self.send_s2c,
            #[cfg(not(target_family = "wasm"))]
            self.send_rtt,
        )
        .await
        {
            Ok(()) => debug!("Closed backend"),
            Err(err) => {
                debug!("Closed backend: {:#}", aeronet::util::pretty_error(&err));
                let _ = self.send_err.send(err);
            }
        }
    }
}

async fn try_handle_connection(
    conn: Connection,
    mut recv_c2s: mpsc::UnboundedReceiver<Bytes>,
    mut send_s2c: mpsc::Sender<Bytes>,
    #[cfg(not(target_family = "wasm"))] mut send_rtt: mpsc::Sender<Duration>,
) -> Result<(), BackendError> {
    debug!("Starting connection loop");
    // in `futures::select!`, if you use `()` the macro breaks
    #[allow(clippy::ignored_unit_patterns)]
    loop {
        // if we failed to send, then buffer's probably full
        // but we don't care, RTT is a lossy bit of info anyway
        #[cfg(not(target_family = "wasm"))]
        let _ = send_rtt.try_send(conn.0.rtt());

        futures::select! {
            result = conn.receive_datagram().fuse() => {
                // OMG WTF ERROR HERE!!! RangeError: supplied view is not large enough.
                let datagram = result.map_err(|err| BackendError::LostConnection(err.into()))?;
                let _ = send_s2c.send(to_bytes(datagram)).await;
            }
            msg = recv_c2s.next() => {
                let Some(msg) = msg else {
                    // frontend closed
                    return Ok(());
                };
                conn.send_datagram(msg).await.map_err(|err| BackendError::SendDatagram(err.into()))?;
            }
        }
    }
}

// optimization: avoid as much reallocation as possible
// * wtransport: use the `wtransport::Datagram::payload -> Bytes`
// * web-sys: use the `Vec<u8>` directly
// TODO upstream this to xwt

#[cfg(target_family = "wasm")]
fn to_bytes(datagram: Vec<u8>) -> Bytes {
    Bytes::from(datagram)
}

#[cfg(not(target_family = "wasm"))]
fn to_bytes(datagram: xwt::current::Datagram) -> Bytes {
    datagram.0.payload()
}
