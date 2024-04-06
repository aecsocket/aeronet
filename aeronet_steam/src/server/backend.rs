use std::sync::Arc;

use aeronet::client;
use ahash::AHashMap;
use bytes::Bytes;
use derivative::Derivative;
use futures::{
    channel::{mpsc, oneshot},
    lock::Mutex,
    never::Never,
    SinkExt, StreamExt,
};
use steamworks::{
    networking_sockets::{ListenSocket, NetConnection, NetPollGroup},
    networking_types::{
        ConnectedEvent, ConnectionRequest, DisconnectedEvent, ListenSocketEvent, NetConnectionEnd,
        NetworkingIdentity, NetworkingMessage,
    },
    SteamId,
};
use tracing::{debug, debug_span, trace, trace_span, warn, warn_span, Instrument};

use crate::{
    server::{BackendError, ClientKey, ConnectionResponse, ListenTarget},
    transport::ConnectionStats,
};

#[derive(Debug)]
pub enum Signal {
    Connecting {
        steam_id: SteamId,
    },
    Connected {
        steam_id: SteamId,
    },
    Disconnected {
        steam_id: SteamId,
    },
    Stats {
        steam_id: SteamId,
        stats: ConnectionStats,
    },
    Recv {
        steam_id: SteamId,
        packet: Bytes,
    },
}

#[derive(Debug)]
pub struct Open {
    pub send_poll: mpsc::Sender<()>,
    pub recv_connecting: mpsc::Receiver<SteamId>,
    pub recv_connected: mpsc::Receiver<SteamId>,
    pub recv_disconnected: mpsc::Receiver<SteamId>,
    pub recv_c2s: mpsc::Receiver<(SteamId, Bytes)>,
    pub send_s2c: mpsc::UnboundedSender<(SteamId, Bytes)>,
    pub recv_stats: mpsc::Receiver<(SteamId, ConnectionStats)>,
}

#[derive(Debug)]
pub struct Connecting {
    pub steam_id: SteamId,
    pub send_key: oneshot::Sender<ClientKey>,
    pub send_conn_resp: oneshot::Sender<ConnectionResponse>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
enum Client<M> {
    Connecting {
        send_internal: oneshot::Sender<(NetConnection<M>, mpsc::UnboundedReceiver<Bytes>)>,
    },
    Connected {
        #[derivative(Debug = "ignore")]
        conn: NetConnection<M>,
    },
}

type ClientMap<M> = Arc<Mutex<AHashMap<SteamId, Client<M>>>>;

const BUFFER_SIZE: usize = 32;

pub async fn open<M: Send + Sync + 'static>(
    steam: steamworks::Client<M>,
    target: ListenTarget,
    recv_batch_size: usize,
    send_open: oneshot::Sender<Open>,
) -> Result<Never, BackendError> {
    // opening
    debug!("Opening server");
    let socks = steam.networking_sockets();
    let sock = match target {
        ListenTarget::Ip(addr) => socks.create_listen_socket_ip(addr, []),
        ListenTarget::Peer { virtual_port } => socks.create_listen_socket_p2p(virtual_port, []),
    }
    .map_err(|_| BackendError::CreateListenSocket)?;
    let mut poll_group = socks.create_poll_group();

    // open
    let (send_poll, mut recv_poll) = mpsc::channel::<()>(1);
    let (send_connecting, recv_connecting) = mpsc::channel::<Connecting>(BUFFER_SIZE);
    let (send_s2c, mut recv_s2c) = mpsc::unbounded::<(SteamId, Bytes)>();
    let (send_flush, mut recv_flush) = mpsc::channel::<()>(1);
    let clients = ClientMap::new(Mutex::new(AHashMap::new()));
    send_open
        .send(Open {
            send_poll,
            recv_connecting,
        })
        .map_err(|_| BackendError::FrontendClosed)?;

    debug!("Started connection loop");
    let mut send_buf = Vec::new();
    loop {
        futures::select! {
            r = recv_poll.next() => {
                r.ok_or(BackendError::FrontendClosed)?;

                while let Some(event) = sock.try_receive_event() {
                    match event {
                        ListenSocketEvent::Connecting(req) => {
                            tokio::spawn(on_connecting(send_connecting.clone(), clients.clone(), req));
                        }
                        ListenSocketEvent::Connected(event) => {
                            on_connected(event, &clients, &poll_group).await;
                        }
                        ListenSocketEvent::Disconnected(event) => {
                            on_disconnected::<M>(event, &clients).await;
                        }
                    }
                }

                recv(&clients, &mut poll_group, recv_batch_size).await?;
            }
            r = recv_s2c.next() => {
                let (steam_id, packet) = r.ok_or(BackendError::FrontendClosed)?;
                buffer_send(&steam, &mut send_buf, steam_id, packet);
            }
            r = recv_flush.next() => {
                r.ok_or(BackendError::FrontendClosed)?;
                flush(&sock, &mut send_buf)?;
            }
        }
    }
}

fn buffer_send<M: Send + Sync + 'static>(
    steam: &steamworks::Client<M>,
    send_buf: &mut Vec<NetworkingMessage<M>>,
    steam_id: SteamId,
    packet: Bytes,
) {
    let mut steam_msg = steam.networking_utils().allocate_message(packet.len());
    // CORRECTNESS: This will error if data has already been set,
    // but we've just allocated this message, so it has no data yet
    steam_msg.set_data(Vec::from(packet)).unwrap();
    steam_msg.set_identity_peer(NetworkingIdentity::new_steam_id(steam_id));
    send_buf.push(steam_msg);
}

fn flush<M: Send + Sync + 'static>(
    sock: &ListenSocket<M>,
    send_buf: &mut Vec<NetworkingMessage<M>>,
) -> Result<(), BackendError> {
    for result in sock.send_messages(send_buf.drain(..)) {
        result.map_err(BackendError::Send)?;
    }
    Ok(())
}

async fn on_connecting<M: Send + Sync + 'static>(
    mut send_connecting: mpsc::Sender<Connecting>,
    mut clients: ClientMap<M>,
    req: ConnectionRequest<M>,
) -> Result<(), BackendError> {
    // if `req` is dropped, the underlying connection is rejected

    let remote = req.remote();
    let Some(steam_id) = remote.steam_id() else {
        debug!("Client with non-Steam ID identity {remote:?} attempted to connect");
        return Ok(());
    };
    debug!("Incoming client with Steam ID {steam_id:?}");

    // get the frontend to generate a new key for this client for us
    // now we can share info about the same client on the frontend and backend
    let (send_key, recv_key) = oneshot::channel::<ClientKey>();
    let (send_conn_resp, recv_conn_resp) = oneshot::channel::<ConnectionResponse>();
    send_connecting
        .send(Connecting {
            steam_id,
            send_key,
            send_conn_resp,
        })
        .await
        .map_err(|_| BackendError::FrontendClosed)?;
    let client_key = recv_key.await.map_err(|_| BackendError::FrontendClosed)?;

    handle_connection(&mut clients, recv_conn_resp, req, steam_id)
        .instrument(debug_span!(
            "Session",
            client = tracing::field::display(client_key)
        ))
        .await?;

    clients.lock().await.remove(&steam_id);
    Ok(())
}

async fn handle_connection<M: Send + Sync + 'static>(
    clients: &mut ClientMap<M>,
    recv_conn_resp: oneshot::Receiver<ConnectionResponse>,
    req: ConnectionRequest<M>,
    steam_id: SteamId,
) -> Result<(), BackendError> {
    debug!("Tracking new client with ID {steam_id:?}");

    // wait for the frontend to determine if it wants to accept this client
    if let ConnectionResponse::Rejected = recv_conn_resp
        .await
        .map_err(|_| BackendError::FrontendClosed)?
    {
        debug!("Rejected client");
        return Ok(());
    }

    // client got accepted; add it in
    let (send_internal, recv_internal) =
        oneshot::channel::<(NetConnection<M>, mpsc::UnboundedReceiver<Bytes>)>();
    clients
        .lock()
        .await
        .insert(steam_id, Client::Connecting { send_internal });
    // only `req.accept()` *after* we've added the oneshot to the connecting map
    // so that `on_connected` is guaranteed to find `steam_id` in the map
    req.accept().map_err(BackendError::AcceptClient)?;
    debug!("Accepted client");
}

async fn on_connected<M: Send + Sync + 'static>(
    event: ConnectedEvent<M>,
    clients: &ClientMap<M>,
    poll_group: &NetPollGroup<M>,
) {
    let remote = event.remote();
    let Some(steam_id) = remote.steam_id() else {
        warn!("Client with non-Steam ID identity {remote:?} attempted to connect");
        return;
    };

    let mut clients = clients.lock().await;
    if let Some(client) = clients.remove(&steam_id) {
        match client {
            Client::Connecting {
                send_internal,
                send_connected,
            } => {
                let (send_c2s, recv_c2s) = mpsc::channel(BUFFER_SIZE);
                let (send_s2c, recv_s2c) = mpsc::unbounded();
                let conn = event.take_connection();
                // set the poll group here instead of in the handler code
                // because we don't have access to `poll_group` from the handler
                // and we don't want to Arc it because we need a mut ref to the poll
                // group to receive messages on it
                conn.set_poll_group(poll_group);
                let _ = send_internal.send((conn, recv_s2c));

                // and give the frontend the new channels to use for send/recv messages
                clients.insert(steam_id, Client::Connected { send_c2s });
                let _ = send_connected.send(Connected { send_s2c, recv_c2s });
            }
            client => {
                clients.insert(steam_id, client);
            }
        }
    }
}

async fn on_disconnected<M: Send + Sync + 'static>(
    event: DisconnectedEvent,
    clients: &ClientMap<M>,
) {
    let remote = event.remote();
    let Some(steam_id) = remote.steam_id() else {
        warn!("Client with non-Steam ID identity {remote:?} attempted to disconnect");
        return;
    };

    clients.lock().await.remove(&steam_id);
}

async fn recv<M: Send + Sync + 'static>(
    clients: &ClientMap<M>,
    poll_group: &mut NetPollGroup<M>,
    recv_batch_size: usize,
) -> Result<(), BackendError> {
    let mut clients = clients.lock().await;
    for packet in poll_group.receive_messages(recv_batch_size) {
        let remote = packet.identity_peer();
        let Some(steam_id) = remote.steam_id() else {
            warn!("Received packet from client with non-Steam ID identity {remote:?}");
            continue;
        };
        let Some(Client::Connected { send_c2s, .. }) = clients.get_mut(&steam_id) else {
            warn!("Received packet from client with ID {steam_id:?} which is not connected");
            continue;
        };
        send_c2s
            .send(Bytes::from(packet.data().to_vec()))
            .await
            .map_err(|_| BackendError::FrontendClosed)?;
    }
    Ok(())
}
