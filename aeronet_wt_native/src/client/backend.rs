use aeronet::{ChannelKey, Message, OnChannel, TryFromBytes, TryIntoBytes};
use tokio::sync::{mpsc, oneshot};
use tracing::debug;
use wtransport::{ClientConfig, Endpoint};

use crate::{shared, EndpointInfo};

use super::{ConnectedClient, ConnectedClientResult, WebTransportError};

pub(super) async fn start<C2S, S2C, C>(
    config: ClientConfig,
    url: String,
    send_connected: oneshot::Sender<ConnectedClientResult<C2S, S2C, C>>,
) where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    let endpoint = match Endpoint::client(config).map_err(WebTransportError::Endpoint) {
        Ok(endpoint) => endpoint,
        Err(err) => {
            let _ = send_connected.send(Err(err));
            return;
        }
    };
    debug!("Created endpoint");

    let conn = match endpoint
        .connect(url.clone())
        .await
        .map_err(WebTransportError::Connect)
    {
        Ok(conn) => conn,
        Err(err) => {
            let _ = send_connected.send(Err(err));
            return;
        }
    };
    debug!("Connected to {url}");

    let channels_state = match shared::establish_channels::<C2S, S2C, C, false>(&conn).await {
        Ok(state) => state,
        Err(err) => {
            let _ = send_connected.send(Err(err));
            return;
        }
    };
    debug!("Established channels");

    let (send_c2s, recv_c2s) = mpsc::unbounded_channel();
    let (send_s2c, recv_s2c) = mpsc::unbounded_channel();
    let (send_info, recv_info) = mpsc::unbounded_channel();
    let (send_err, recv_err) = oneshot::channel();
    let connected = ConnectedClient::<C2S, S2C, C> {
        local_addr: endpoint.local_addr(),
        info: EndpointInfo::from_connection(&conn),
        recv_info,
        recv_s2c,
        send_c2s,
        recv_err,
    };
    if send_connected.send(Ok(connected)).is_err() {
        debug!("Frontend closed");
        return;
    }

    debug!("Starting connection loop");
    if let Err(err) = shared::handle_connection::<C2S, S2C, C>(
        conn,
        channels_state,
        send_info,
        send_s2c,
        recv_c2s,
    )
    .await
    {
        debug!("Disconnected with error");
        let _ = send_err.send(err);
    } else {
        debug!("Disconnected without error");
    }
}
