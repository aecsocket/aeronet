mod negotiate;

use aeronet_protocol::lane::Lanes;
use derivative::Derivative;
use tracing::debug;
use xwt::current::{Connection, RecvStream, SendStream};
use xwt_core::datagram::{Receive, Send};

use std::time::Duration;

use aeronet::{LaneConfig, LaneIndex, OnLane, ProtocolVersion, TryAsBytes, TryFromBytes};
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    FutureExt, SinkExt, StreamExt,
};

use crate::{BackendError, ConnectionInfo, MessageKey, WebTransportError};

const MSG_BUF_CAP: usize = 64;

#[derive(Debug)]
pub struct ConnectionFrontend {
    pub info: ConnectionInfo,
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_s2c: mpsc::Receiver<Bytes>,
    recv_rtt: mpsc::Receiver<Duration>,
    recv_err: oneshot::Receiver<BackendError>,
    lanes: Lanes,
    _send_closed: oneshot::Sender<()>,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct ConnectionBackend {
    recv_c2s: mpsc::UnboundedReceiver<Bytes>,
    send_s2c: mpsc::Sender<Bytes>,
    send_rtt: mpsc::Sender<Duration>,
    send_err: oneshot::Sender<BackendError>,
    recv_closed: oneshot::Receiver<()>,
    #[derivative(Debug = "ignore")]
    _send_managed: SendStream,
    #[derivative(Debug = "ignore")]
    _recv_managed: RecvStream,
}

pub async fn connection_channel<const SERVER: bool>(
    conn: &mut Connection,
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
    let (send_closed, recv_closed) = oneshot::channel();
    Ok((
        ConnectionFrontend {
            info: ConnectionInfo::from(&*conn),
            send_c2s,
            recv_s2c,
            recv_rtt,
            recv_err,
            lanes: Lanes::new(max_packet_len, lanes),
            _send_closed: send_closed,
        },
        ConnectionBackend {
            recv_c2s,
            send_s2c,
            send_rtt,
            send_err,
            recv_closed,
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

    pub fn buffer_send<S: TryAsBytes + OnLane, R: TryFromBytes>(
        &mut self,
        msg: &S,
    ) -> Result<MessageKey, WebTransportError<S, R>> {
        let msg_bytes = msg.try_as_bytes().map_err(WebTransportError::AsBytes)?;
        let msg_bytes = msg_bytes.as_ref();

        let seq = self
            .lanes
            .buffer_send(msg.lane().index(), msg_bytes)
            .map_err(BackendError::LaneSend)?;

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

    pub fn recv<S: TryAsBytes, R: TryFromBytes>(
        &mut self,
    ) -> Result<Option<R>, WebTransportError<S, R>> {
        while let Ok(Some(packet)) = self.recv_s2c.try_next() {
            self.info.total_bytes_recv += packet.len();
            if let Some(msg_bytes) = self
                .lanes
                .recv(&packet)
                .map_err(|err| BackendError::LaneRecv(err))?
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
        // This fn handles receiving and sending datagrams. While we could impl
        // this as a `futures::select!`, this won't actually work on WASM
        // because `receive_datagram` is not cancel-safe.
        // So instead, we split this up into two async tasks, and run them both.

        debug!("Connected backend");
        let fut_incoming = connection_incoming(
            &conn,
            self.send_s2c,
            #[cfg(not(target_family = "wasm"))]
            self.send_rtt,
        );
        let fut_outgoing = connection_outgoing(&conn, self.recv_c2s);
        match futures::select! {
            r = fut_incoming.fuse() => r,
            r = fut_outgoing.fuse() => r,
            _ = self.recv_closed => Ok(()),
        } {
            Ok(_) => {
                #[cfg(target_family = "wasm")]
                {
                    let mut close_info = web_sys::WebTransportCloseInfo::new();
                    close_info.close_code(10);
                    conn.transport.close_with_close_info(&close_info);
                    // wait for the closing info to be sent
                    // otherwise the peer will just timeout instead of cleanly close
                    // TODO: this doesn't actually work. maybe because xwt manually calls close
                    // on drop?
                    let _ = wasm_bindgen_futures::JsFuture::from(conn.transport.closed()).await;
                }
                debug!("Closed backend");
            }
            Err(err) => {
                debug!("Closed backend: {:#}", aeronet::util::pretty_error(&err));
                let _ = self.send_err.send(err);
            }
        }
    }
}

async fn connection_incoming(
    conn: &Connection,
    mut send_s2c: mpsc::Sender<Bytes>,
    #[cfg(not(target_family = "wasm"))] mut send_rtt: mpsc::Sender<Duration>,
) -> Result<(), BackendError> {
    loop {
        // if we failed to send, then buffer's probably full
        // but we don't care, RTT is a lossy bit of info anyway
        #[cfg(not(target_family = "wasm"))]
        if let Err(err) = send_rtt.try_send(conn.0.rtt()) {
            if err.is_disconnected() {
                // frontend closed
                return Ok(());
            }
        }

        #[allow(clippy::useless_conversion)] // multi-target support
        let datagram = conn
            .receive_datagram()
            .await
            .map_err(|err| BackendError::RecvDatagram(err.into()))?;
        if send_s2c.send(to_bytes(datagram)).await.is_err() {
            // backend closed
            return Ok(());
        }
    }
}

async fn connection_outgoing(
    conn: &Connection,
    mut recv_c2s: mpsc::UnboundedReceiver<Bytes>,
) -> Result<(), BackendError> {
    loop {
        let Some(msg) = recv_c2s.next().await else {
            // backend closed
            return Ok(());
        };

        #[allow(clippy::useless_conversion)] // multi-target support
        conn.send_datagram(msg)
            .await
            .map_err(|err| BackendError::SendDatagram(err.into()))?;
    }
}

// optimization: avoid as much reallocation as possible
// * wtransport: use the `wtransport::Datagram::payload -> Bytes`
// * web-sys: use the `Vec<u8>` directly
// TODO upstream this to xwt

#[cfg(target_family = "wasm")]
fn to_bytes(datagram: Vec<u8>) -> Bytes {
    debug_assert_eq!(datagram.capacity(), datagram.len());
    Bytes::from(datagram)
}

#[cfg(not(target_family = "wasm"))]
#[allow(clippy::needless_pass_by_value)] // multi-target support
fn to_bytes(datagram: xwt::current::Datagram) -> Bytes {
    datagram.0.payload()
}
