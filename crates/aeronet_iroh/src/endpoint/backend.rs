use {
    super::{EndpointError, ToConnecting, ToOpen},
    crate::{IrohRuntime, session},
    aeronet_io::connection::DisconnectReason,
    futures::{
        FutureExt, SinkExt, StreamExt,
        channel::{mpsc, oneshot},
        never::Never,
    },
    iroh::endpoint::{Builder, Incoming},
    tracing::{Instrument, debug, debug_span},
};

pub async fn start(
    builder: Builder,
    tx_next: oneshot::Sender<ToOpen>,
) -> Result<Never, EndpointError> {
    debug!("Spawning backend task to open endpoint");

    let endpoint = builder.bind().await.map_err(EndpointError::Bind)?;
    debug!(id = %endpoint.id(), "Opened endpoint");

    let (tx_connecting, rx_connecting) = mpsc::channel(1);
    let (tx_dropped, mut rx_dropped) = mpsc::channel::<()>(0);
    tx_next
        .send(ToOpen {
            endpoint: endpoint.clone(),
            rx_connecting,
            tx_dropped,
        })
        .map_err(|_| EndpointError::FrontendClosed)?;

    debug!("Starting endpoint accept loop");
    loop {
        let incoming = futures::select! {
            incoming = endpoint.accept().fuse() => incoming,
            _ = rx_dropped.next() => {
                endpoint.close().await;
                return Err(EndpointError::FrontendClosed);
            }
        }
        .ok_or(EndpointError::Closed)?;

        IrohRuntime::spawn({
            let tx_connecting = tx_connecting.clone();
            async move {
                if let Err(err) = accept_session(incoming, tx_connecting).await {
                    debug!("Failed to accept session: {err:?}");
                }
            }
        });
    }
}

async fn accept_session(
    incoming: Incoming,
    mut tx_connecting: mpsc::Sender<ToConnecting>,
) -> Result<(), DisconnectReason> {
    let conn = incoming.await.map_err(session::SessionError::Accept)?;
    let peer_id = conn.remote_id();

    let (tx_response, rx_response) = oneshot::channel();
    let (tx_dc_reason, rx_dc_reason) = oneshot::channel();
    let (tx_next, rx_next) = oneshot::channel();
    tx_connecting
        .send(ToConnecting {
            peer_id,
            tx_response,
            rx_dc_reason,
            rx_next,
        })
        .await
        .map_err(|_| session::SessionError::FrontendClosed)?;

    let Err(reason) = session::start_incoming(conn, rx_response, tx_next)
        .instrument(debug_span!("session", %peer_id, side = "incoming"))
        .await;
    _ = tx_dc_reason.send(reason);
    Ok(())
}
