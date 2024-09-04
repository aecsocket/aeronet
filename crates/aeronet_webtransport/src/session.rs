use std::{io, net::SocketAddr, num::Saturating, sync::Arc, time::Duration};

use aeronet::{
    io::{PacketBuffers, PacketMtu},
    session::SessionSet,
    stats::{RemoteAddr, SessionStats},
};
use bevy_app::prelude::*;
use bevy_derive::Deref;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    FutureExt, SinkExt, StreamExt,
};
use thiserror::Error;
use tracing::{debug, trace, trace_span};
use xwt_core::prelude::*;

use crate::runtime::WebTransportRuntime;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        type ConnectionError = ();
    } else {
        use wtransport::error::SendDatagramError;

        type Connection = xwt_wtransport::Connection;
        type ConnectionError = wtransport::error::ConnectionError;
    }
}

#[derive(Debug)]
pub struct WebTransportSessionPlugin;

impl Plugin for WebTransportSessionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WebTransportRuntime>()
            .add_systems(PreUpdate, recv.in_set(SessionSet::Recv))
            .add_systems(PostUpdate, send.in_set(SessionSet::Send));
    }
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("frontend closed")]
    FrontendClosed,
    #[error("backend closed")]
    BackendClosed,
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to get local socket address")]
    GetLocalAddr(#[source] io::Error),
    #[error("datagrams not supported")]
    DatagramsNotSupported,
    #[error("connection lost")]
    Connection(#[source] ConnectionError),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deref, Component, Reflect)]
#[reflect(Component)]
pub struct RawRtt(pub(crate) Duration);

#[derive(Debug, Component)]
pub(crate) struct WebTransportIo {
    pub(crate) recv_err: oneshot::Receiver<anyhow::Error>,
    pub(crate) recv_meta: mpsc::Receiver<SessionMeta>,
    pub(crate) recv_packet_b2f: mpsc::Receiver<Bytes>,
    pub(crate) send_packet_f2b: mpsc::UnboundedSender<Bytes>,
}

#[derive(Debug)]
pub(crate) struct SessionMeta {
    #[cfg(not(target_family = "wasm"))]
    remote_addr: SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    raw_rtt: Duration,
    mtu: usize,
}

pub(crate) const PACKET_BUF_CAP: usize = 16;

const META_UPDATE_INTERVAL: Duration = Duration::from_millis(100);

fn recv(
    mut query: Query<(
        Entity,
        &mut WebTransportIo,
        &mut PacketBuffers,
        &mut PacketMtu,
        &mut SessionStats,
        Option<&mut RemoteAddr>,
        Option<&mut RawRtt>,
    )>,
) {
    for (session, mut io, mut bufs, mut mtu, mut stats, mut remote_addr, mut raw_rtt) in &mut query
    {
        let span = trace_span!("recv", ?session);
        let _span = span.enter();

        while let Ok(Some(meta)) = io.recv_meta.try_next() {
            **mtu = meta.mtu;
            #[cfg(not(target_family = "wasm"))]
            {
                if let Some(ref mut remote_addr) = remote_addr {
                    remote_addr.0 = meta.remote_addr;
                }

                if let Some(ref mut raw_rtt) = raw_rtt {
                    raw_rtt.0 = meta.raw_rtt;
                }
            }
        }

        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        while let Ok(Some(packet)) = io.recv_packet_b2f.try_next() {
            num_packets += 1;
            stats.packets_recv += 1;

            num_bytes += packet.len();
            stats.bytes_recv += packet.len();

            bufs.recv.push(packet);
        }

        trace!(
            num_packets = num_packets.0,
            num_bytes = num_bytes.0,
            "Received packets",
        );
    }
}

fn send(
    mut query: Query<(
        Entity,
        &WebTransportIo,
        &mut PacketBuffers,
        &mut SessionStats,
    )>,
) {
    for (session, io, mut bufs, mut stats) in &mut query {
        let span = trace_span!("send", ?session);
        let _span = span.enter();

        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        for packet in bufs.send.drain(..) {
            num_packets += 1;
            stats.packets_sent += 1;

            num_bytes += packet.len();
            stats.bytes_sent += packet.len();

            // handle errors in `recv`
            let _ = io.send_packet_f2b.unbounded_send(packet);
        }

        trace!(
            num_packets = num_packets.0,
            num_bytes = num_bytes.0,
            "Sent packets",
        );
    }
}

#[derive(Debug)]
pub(crate) struct SessionBackend {
    pub(crate) runtime: WebTransportRuntime,
    pub(crate) conn: Connection,
    pub(crate) send_meta: mpsc::Sender<SessionMeta>,
    pub(crate) recv_packet_f2b: mpsc::UnboundedReceiver<Bytes>,
    pub(crate) send_packet_b2f: mpsc::Sender<Bytes>,
}

impl SessionBackend {
    pub async fn start(self) -> Result<Never, SessionError> {
        let SessionBackend {
            runtime,
            conn,
            send_meta,
            recv_packet_f2b,
            send_packet_b2f,
        } = self;

        let conn = Arc::new(conn);
        let (send_err, mut recv_err) = mpsc::channel::<SessionError>(1);

        let (_send_meta_closed, recv_meta_closed) = oneshot::channel();
        runtime.spawn({
            let runtime = runtime.clone();
            let conn = conn.clone();
            let mut send_err = send_err.clone();
            async move {
                let Err(err) = meta_loop(runtime, conn, recv_meta_closed, send_meta).await else {
                    unreachable!();
                };
                let _ = send_err.try_send(err);
            }
        });

        let (_send_sending_closed, recv_sending_closed) = oneshot::channel();
        runtime.spawn({
            let conn = conn.clone();
            let mut send_err = send_err.clone();
            async move {
                let Err(err) = send_loop(conn, recv_sending_closed, recv_packet_f2b).await else {
                    unreachable!();
                };
                let _ = send_err.try_send(err);
            }
        });

        let (_send_receiving_closed, recv_receiving_closed) = oneshot::channel();
        runtime.spawn({
            let conn = conn.clone();
            let mut send_err = send_err.clone();
            async move {
                let Err(err) = recv_loop(conn, recv_receiving_closed, send_packet_b2f).await else {
                    unreachable!();
                };
                let _ = send_err.try_send(err);
            }
        });

        let err = futures::select! {
            err = recv_err.next() => {
                err.unwrap_or(SessionError::BackendClosed)
            }
        };

        loop {}
    }
}

async fn meta_loop(
    runtime: WebTransportRuntime,
    conn: Arc<Connection>,
    mut recv_closed: oneshot::Receiver<()>,
    mut send_meta: mpsc::Sender<SessionMeta>,
) -> Result<Never, SessionError> {
    loop {
        futures::select! {
            () = runtime.sleep(META_UPDATE_INTERVAL).fuse() => {},
            _ = recv_closed => return Err(SessionError::FrontendClosed),
        };

        let meta = SessionMeta {
            #[cfg(not(target_family = "wasm"))]
            remote_addr: conn.0.remote_address(),
            #[cfg(not(target_family = "wasm"))]
            raw_rtt: conn.0.rtt(),
            mtu: conn
                .max_datagram_size()
                .ok_or(SessionError::DatagramsNotSupported)?,
        };
        match send_meta.try_send(meta) {
            Ok(_) => {}
            Err(err) if err.is_full() => {}
            Err(_) => {
                return Err(SessionError::FrontendClosed);
            }
        }
    }
}

async fn recv_loop(
    conn: Arc<Connection>,
    mut recv_closed: oneshot::Receiver<()>,
    mut send_packet_b2f: mpsc::Sender<Bytes>,
) -> Result<Never, SessionError> {
    loop {
        #[allow(clippy::useless_conversion)] // multi-target support
        let packet = futures::select! {
            x = conn.receive_datagram().fuse() => x,
            _ = recv_closed => return Err(SessionError::FrontendClosed),
        }
        .map_err(|err| SessionError::Connection(err.into()))?;

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

        send_packet_b2f
            .send(packet)
            .await
            .map_err(|_| SessionError::BackendClosed)?;
    }
}

async fn send_loop(
    conn: Arc<Connection>,
    mut recv_closed: oneshot::Receiver<()>,
    mut recv_packet_f2b: mpsc::UnboundedReceiver<Bytes>,
) -> Result<Never, SessionError> {
    loop {
        let packet = futures::select! {
            x = recv_packet_f2b.next() => x,
            _ = recv_closed => return Err(SessionError::FrontendClosed),
        }
        .ok_or(SessionError::FrontendClosed)?;

        #[cfg(target_family = "wasm")]
        {
            conn.send_datagram(packet)
                .await
                .map_err(|err| InternalError::ConnectionLost(err.into()))?;
        }

        #[cfg(not(target_family = "wasm"))]
        {
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
                    let mtu = conn.max_datagram_size();
                    debug!(
                        packet_len,
                        mtu, "Attempted to send datagram larger than MTU"
                    );
                    Ok(())
                }
                Err(SendDatagramError::UnsupportedByPeer) => {
                    // this should be impossible, since we checked that the client does support
                    // datagrams before connecting, but we'll error-case it anyway
                    Err(SessionError::DatagramsNotSupported)
                }
            }?;
        }
    }
}
