use aeronet::error::pretty_error;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    FutureExt, SinkExt, StreamExt,
};
use octs::Bytes;
use slotmap::SlotMap;
use tracing::{debug, debug_span, Instrument};
use wtransport::endpoint::{IncomingSession, SessionRequest};

use crate::{internal, shared::ConnectionStats};

use super::{ClientKey, Connected, Connecting, ConnectionResponse, Open, ServerError};

pub async fn start(
    config: wtransport::ServerConfig,
    send_open: oneshot::Sender<Open>,
) -> Result<Never, ServerError> {
    let endpoint = wtransport::Endpoint::server(config).map_err(ServerError::CreateEndpoint)?;
    let local_addr = endpoint.local_addr().map_err(ServerError::GetLocalAddr)?;

    let (send_closed, mut recv_closed) = oneshot::channel::<()>();
    let (send_connecting, recv_connecting) = mpsc::channel::<Connecting>(4);
    send_open
        .send(Open {
            local_addr,
            recv_connecting,
            clients: SlotMap::default(),
            _send_closed: send_closed,
        })
        .map_err(|_| ServerError::FrontendClosed)?;

    loop {
        let session = futures::select! {
            _ = recv_closed => return Err(ServerError::FrontendClosed),
            x = endpoint.accept().fuse() => x,
        };
        let send_connecting = send_connecting.clone();
        tokio::spawn(async move {
            if let Err(err) = start_handle_session(send_connecting, session).await {
                debug!("Failed to start handling session: {:#}", pretty_error(&err));
            }
        });
    }
}

async fn start_handle_session(
    mut send_connecting: mpsc::Sender<Connecting>,
    session: IncomingSession,
) -> Result<(), ServerError> {
    let req = session.await.map_err(ServerError::AwaitSessionRequest)?;

    let (send_key, recv_key) = oneshot::channel::<ClientKey>();
    let (send_conn_resp, recv_conn_resp) = oneshot::channel::<ConnectionResponse>();
    let (send_err, recv_err) = oneshot::channel::<ServerError>();
    let (send_connected, recv_connected) = oneshot::channel::<Connected>();
    send_connecting
        .send(Connecting {
            authority: req.authority().to_string(),
            path: req.path().to_string(),
            origin: req.origin().map(ToOwned::to_owned),
            user_agent: req.user_agent().map(ToOwned::to_owned),
            headers: req.headers().clone(),
            send_key,
            send_conn_resp,
            recv_err,
            recv_connected,
        })
        .await
        .map_err(|_| ServerError::FrontendClosed)?;
    let client_key = recv_key.await.map_err(|_| ServerError::FrontendClosed)?;

    let err = async move {
        let Err(err) = handle_session(req, recv_conn_resp, send_connected).await else {
            unreachable!()
        };
        match &err {
            ServerError::FrontendClosed => {
                debug!("Session closed");
            }
            err => {
                debug!("Session closed: {:#}", pretty_error(err));
            }
        }
        err
    }
    .instrument(debug_span!(
        "Session",
        client = tracing::field::display(client_key)
    ))
    .await;
    let _ = send_err.send(err);
    Ok(())
}

async fn handle_session(
    req: SessionRequest,
    recv_conn_resp: oneshot::Receiver<ConnectionResponse>,
    send_connected: oneshot::Sender<Connected>,
) -> Result<Never, ServerError> {
    debug!("New session request from {}{}", req.authority(), req.path());

    let conn_resp = recv_conn_resp
        .await
        .map_err(|_| ServerError::FrontendClosed)?;
    debug!("Frontend responded to this request with {conn_resp:?}");

    let conn = match conn_resp {
        ConnectionResponse::Accept => req.accept(),
        ConnectionResponse::Forbidden => {
            req.forbidden().await;
            return Err(ServerError::ForceDisconnect);
        }
        ConnectionResponse::NotFound => {
            req.not_found().await;
            return Err(ServerError::ForceDisconnect);
        }
    }
    .await
    .map_err(ServerError::AcceptSessionRequest)?;

    debug!("Connection opened, forwarding to frontend");

    let (send_c2s, recv_c2s) = mpsc::channel::<Bytes>(internal::MSG_BUF_CAP);
    let (send_s2c, mut recv_s2c) = mpsc::unbounded::<Bytes>();
    let (send_stats, recv_stats) = mpsc::channel::<ConnectionStats>(1);
    send_connected
        .send(Connected {
            remote_addr: conn.remote_address(),
            stats: ConnectionStats {
                rtt: conn.rtt(),
                ..Default::default()
            },
            recv_c2s,
            send_s2c,
            recv_stats,
        })
        .map_err(|_| ServerError::FrontendClosed)?;

    debug!("Starting connection loop");
    let send = async {
        loop {
            let msg = recv_s2c.next().await.ok_or(ServerError::FrontendClosed)?;
            conn.send_datagram(msg).map_err(ServerError::SendDatagram)?;
        }
    };

    todo!()
}
