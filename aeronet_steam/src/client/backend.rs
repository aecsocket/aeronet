use aeronet::protocol::ProtocolVersion;
use aeronet_proto::negotiate;
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    SinkExt, StreamExt,
};
use steamworks::{
    networking_sockets::{NetConnection, NetworkingSockets},
    networking_types::{
        NetConnectionStatusChanged, NetworkingConnectionState, NetworkingIdentity, SendFlags,
    },
    SteamError,
};

use crate::transport::ConnectionStats;

use super::ConnectTarget;

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    #[error("invalid handle")]
    InvalidHandle,
    #[error("frontend closed")]
    FrontendClosed,

    #[error("connection rejected by peer")]
    Rejected,
    #[error("connection failed")]
    Failed,
    #[error("failed to send negotiation request")]
    SendNegotiate(#[source] SteamError),
    #[error("failed to negotiate protocol")]
    Negotiate(#[source] negotiate::ResponseError),

    #[error("failed to send message")]
    Send(#[source] SteamError),
}

#[derive(Debug)]
pub(super) struct Negotiating {
    pub send_poll: mpsc::Sender<()>,
    pub recv_connected: oneshot::Receiver<Connected>,
}

#[derive(Debug)]
pub(super) struct Connected {
    pub recv_stats: mpsc::Receiver<ConnectionStats>,
    pub recv_s2c: mpsc::Receiver<Bytes>,
    pub send_c2s: mpsc::UnboundedSender<Bytes>,
}

pub(super) async fn open<M: Send + Sync + 'static>(
    steam: steamworks::Client<M>,
    target: ConnectTarget,
    version: ProtocolVersion,
    recv_batch_size: usize,
    send_negotiating: oneshot::Sender<Negotiating>,
) -> Result<Never, Error> {
    struct Callback<M>(steamworks::CallbackHandle<M>);

    impl<M> Drop for Callback<M> {
        fn drop(&mut self) {
            self.0.disconnect()
        }
    }

    let socks = steam.networking_sockets();
    let mut conn = match target {
        ConnectTarget::Ip(addr) => socks.connect_by_ip_address(addr, []),
        ConnectTarget::Peer { id, virtual_port } => {
            socks.connect_p2p(NetworkingIdentity::new_steam_id(id), virtual_port, [])
        }
    }
    .map_err(|_| Error::InvalidHandle)?;

    let (send_connected, recv_connected) = oneshot::channel();
    let _connection_changed_cb =
        Callback(steam.register_callback(connection_changed_cb(send_connected)));
    recv_connected
        .await
        .map_err(|_| Error::Failed)
        .and_then(|r| r)?;

    // negotiating
    let (send_poll, mut recv_poll) = mpsc::channel::<()>(1);
    let (send_connected, recv_connected) = oneshot::channel();
    send_negotiating
        .send(Negotiating {
            send_poll,
            recv_connected,
        })
        .map_err(|_| Error::FrontendClosed)?;
    assert_send(negotiate(version, &mut conn, &mut recv_poll)).await?;

    // connected
    let (mut send_stats, recv_stats) = mpsc::channel::<ConnectionStats>(1);
    let (mut send_s2c, recv_s2c) = mpsc::channel::<Bytes>(32);
    let (send_c2s, mut recv_c2s) = mpsc::unbounded::<Bytes>();
    send_connected
        .send(Connected {
            recv_stats,
            recv_s2c,
            send_c2s,
        })
        .map_err(|_| Error::FrontendClosed)?;

    loop {
        assert_send(connection_loop(
            &socks,
            &mut conn,
            recv_batch_size,
            &mut recv_poll,
            &mut send_stats,
            &mut send_s2c,
            &mut recv_c2s,
        ))
        .await?;
    }
}

fn connection_changed_cb(
    send_connected: oneshot::Sender<Result<(), Error>>,
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
                let _ = s.send(Err(Error::Rejected));
            }
        }
        NetworkingConnectionState::None | NetworkingConnectionState::ProblemDetectedLocally => {
            if let Some(s) = send_connected.take() {
                let _ = s.send(Err(Error::Failed));
            }
        }
    }
}

async fn negotiate<M: Send + Sync + 'static>(
    version: ProtocolVersion,
    conn: &mut NetConnection<M>,
    recv_poll: &mut mpsc::Receiver<()>,
) -> Result<(), Error> {
    let negotiate = negotiate::Negotiation::new(version);
    conn.send_message(&negotiate.request(), SendFlags::RELIABLE_NO_NAGLE)
        .map_err(Error::SendNegotiate)?;

    let msg = loop {
        recv_poll.next().await.ok_or(Error::FrontendClosed)?;

        if let Some(msg) = conn
            .receive_messages(1)
            .map_err(|_| Error::InvalidHandle)?
            .into_iter()
            .next()
        {
            break msg;
        }
    };

    negotiate
        .recv_response(msg.data())
        .map_err(Error::Negotiate)
}

async fn connection_loop<M: Send + Sync + 'static>(
    socks: &NetworkingSockets<M>,
    conn: &mut NetConnection<M>,
    recv_batch_size: usize,
    recv_poll: &mut mpsc::Receiver<()>,
    send_stats: &mut mpsc::Sender<ConnectionStats>,
    send_s2c: &mut mpsc::Sender<Bytes>,
    recv_c2s: &mut mpsc::UnboundedReceiver<Bytes>,
) -> Result<(), Error> {
    futures::select! {
        msg = recv_c2s.next() => {
            let Some(msg) = msg else {
                // frontend closed
                return Ok(());
            };
            conn.send_message(&msg, SendFlags::UNRELIABLE_NO_NAGLE)
                .map_err(Error::Send)?;
        }
        _ = recv_poll.next() => {
            let _ = send_stats.try_send(ConnectionStats::from_connection(&socks, &conn));
            // can't pass this iterator into `send_all` directly
            // because steamworks message type is !Send
            // so we must allocate an intermediate Vec for the output Bytes :(
            let msgs = conn
                .receive_messages(recv_batch_size)
                .map_err(|_| Error::InvalidHandle)?
                .into_iter()
                .map(|packet| Ok(Bytes::from(packet.data().to_vec())))
                .collect::<Vec<_>>();
            let _ = send_s2c.send_all(&mut futures::stream::iter(msgs)).await;
        }
    }
    Ok(())
}

fn assert_send<T: Send>(t: T) -> T {
    t
}
