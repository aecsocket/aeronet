use aeronet::protocol::ProtocolVersion;
use aeronet_proto::negotiate;
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    SinkExt, StreamExt,
};
use steamworks::{
    networking_sockets::NetConnection,
    networking_types::{
        NetConnectionStatusChanged, NetworkingConnectionState, NetworkingIdentity, SendFlags,
    },
};
use tracing::{debug, trace};

use crate::{
    client::{BackendError, ConnectTarget},
    transport::ConnectionStats,
};

#[derive(Debug)]
pub struct Negotiating {
    pub send_poll: mpsc::Sender<()>,
    pub recv_connected: oneshot::Receiver<Connected>,
}

#[derive(Debug)]
pub struct Connected {
    pub stats: ConnectionStats,
    pub recv_stats: mpsc::Receiver<ConnectionStats>,
    pub recv_s2c: mpsc::Receiver<Bytes>,
    pub send_c2s: mpsc::UnboundedSender<Bytes>,
    pub send_flush: mpsc::Sender<()>,
}

const BUFFER_SIZE: usize = 32;

pub async fn open<M: Send + Sync + 'static>(
    steam: steamworks::Client<M>,
    target: ConnectTarget,
    version: ProtocolVersion,
    recv_batch_size: usize,
    send_negotiating: oneshot::Sender<Negotiating>,
) -> Result<Never, BackendError> {
    struct Callback<M>(steamworks::CallbackHandle<M>);

    impl<M> Drop for Callback<M> {
        fn drop(&mut self) {
            self.0.disconnect()
        }
    }

    // connecting
    debug!("Opening connection to {target:?}");
    let socks = steam.networking_sockets();
    let mut conn = match target {
        ConnectTarget::Ip(addr) => socks.connect_by_ip_address(addr, []),
        ConnectTarget::Peer {
            steam_id,
            virtual_port,
        } => socks.connect_p2p(NetworkingIdentity::new_steam_id(steam_id), virtual_port, []),
    }
    .map_err(|_| BackendError::CreateConnection)?;

    let (send_connected, recv_connected) = oneshot::channel();
    let _connection_changed_cb =
        Callback(steam.register_callback(connection_changed_cb(send_connected)));
    recv_connected
        .await
        .map_err(|_| BackendError::Failed)
        .and_then(|r| r)?;

    // negotiating
    let (send_poll, mut recv_poll) = mpsc::channel::<()>(1);
    let (send_connected, recv_connected) = oneshot::channel();
    send_negotiating
        .send(Negotiating {
            send_poll,
            recv_connected,
        })
        .map_err(|_| BackendError::FrontendClosed)?;
    assert_send(negotiate(version, &mut conn, &mut recv_poll)).await?;

    // connected
    let (mut send_stats, recv_stats) = mpsc::channel::<ConnectionStats>(1);
    let (mut send_s2c, recv_s2c) = mpsc::channel::<Bytes>(BUFFER_SIZE);
    let (send_c2s, mut recv_c2s) = mpsc::unbounded::<Bytes>();
    let (send_flush, mut recv_flush) = mpsc::channel::<()>(1);
    send_connected
        .send(Connected {
            stats: ConnectionStats::from_connection(&socks, &conn),
            recv_stats,
            recv_s2c,
            send_c2s,
            send_flush,
        })
        .map_err(|_| BackendError::FrontendClosed)?;

    debug!("Started connection loop");
    let mut send_buf = Vec::new();
    loop {
        futures::select! {
            packet = recv_c2s.next() => {
                let Some(packet) = packet else {
                    // frontend closed
                    return Err(BackendError::FrontendClosed);
                };
                trace!("Buffered packet of length {} for sending", packet.len());
                send_buf.push(packet);
            }
            _ = recv_flush.next() => {
                trace!("Sent {} buffered packets", send_buf.len());
                for packet in send_buf.drain(..) {
                    conn.send_message(&packet, SendFlags::UNRELIABLE_NO_NAGLE)
                        .map_err(BackendError::Send)?;
                }
            }
            _ = recv_poll.next() => {
                send_stats
                    .try_send(ConnectionStats::from_connection(&socks, &conn))
                    .map_err(|_| BackendError::FrontendClosed)?;
                // can't pass this iterator into `send_all` directly
                // because steamworks message type is !Send
                // so we must allocate an intermediate Vec for the output Bytes
                let packets = conn
                    .receive_messages(recv_batch_size)
                    .map_err(|_| BackendError::InvalidHandle)?
                    .into_iter()
                    .map(|packet| Bytes::from(packet.data().to_vec()))
                    .inspect(|packet| trace!("Received packet of length {}", packet.len()))
                    .map(Ok)
                    .collect::<Vec<_>>();
                send_s2c
                    .send_all(&mut futures::stream::iter(packets)).await
                    .map_err(|_| BackendError::FrontendClosed)?;
            }
        }
    }
}

fn connection_changed_cb(
    send_connected: oneshot::Sender<Result<(), BackendError>>,
) -> impl FnMut(NetConnectionStatusChanged) {
    let mut send_connected = Some(send_connected);
    move |event| match event
        .connection_info
        .state()
        .unwrap_or(NetworkingConnectionState::None)
    {
        NetworkingConnectionState::Connecting | NetworkingConnectionState::FindingRoute => {}
        NetworkingConnectionState::Connected => {
            if let Some(s) = send_connected.take() {
                let _ = s.send(Ok(()));
            }
        }
        NetworkingConnectionState::ClosedByPeer => {
            if let Some(s) = send_connected.take() {
                let _ = s.send(Err(BackendError::Rejected));
            }
        }
        NetworkingConnectionState::None | NetworkingConnectionState::ProblemDetectedLocally => {
            if let Some(s) = send_connected.take() {
                let _ = s.send(Err(BackendError::Failed));
            }
        }
    }
}

async fn negotiate<M: Send + Sync + 'static>(
    version: ProtocolVersion,
    conn: &mut NetConnection<M>,
    recv_poll: &mut mpsc::Receiver<()>,
) -> Result<(), BackendError> {
    let negotiate = negotiate::Negotiation::new(version);
    conn.send_message(&negotiate.request(), SendFlags::RELIABLE_NO_NAGLE)
        .map_err(BackendError::SendNegotiate)?;

    let msg = loop {
        recv_poll.next().await.ok_or(BackendError::FrontendClosed)?;

        if let Some(msg) = conn
            .receive_messages(1)
            .map_err(|_| BackendError::InvalidHandle)?
            .into_iter()
            .next()
        {
            break msg;
        }
    };

    negotiate
        .recv_response(msg.data())
        .map_err(BackendError::Negotiate)?;

    debug!("Negotiated connection on version {version}");
    Ok(())
}

fn assert_send<T: Send>(t: T) -> T {
    t
}
