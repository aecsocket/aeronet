use aeronet::{ChannelKey, Message, OnChannel, TryFromBytes, TryIntoBytes};
use tokio::sync::{mpsc, oneshot};
use tracing::debug;
use wtransport::{ClientConfig, Endpoint};

use crate::{shared, ConnectedClient, ConnectingClient, EndpointInfo, OpenClient};

use super::{OpenResult, WebTransportError};

pub(super) async fn start<C2S, S2C, C>(
    config: ClientConfig,
    send_open: oneshot::Sender<OpenResult<C2S, S2C, C>>,
) where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    debug!("Starting backend");
    let endpoint = match Endpoint::client(config).map_err(WebTransportError::Endpoint) {
        Ok(endpoint) => endpoint,
        Err(err) => {
            let _ = send_open.send(Err(err));
            return;
        }
    };
    debug!("Created endpoint");

    let mut send_open = send_open;
    loop {
        let (send_url, recv_url) = oneshot::channel();
        let (send_connecting, recv_connecting) = oneshot::channel();
        let open = OpenClient {
            send_url,
            recv_connecting,
        };
        if let Err(_) = send_open.send(Ok(open)) {
            debug!("Frontend closed");
            return;
        }

        let (new_send_open, recv_open) = oneshot::channel();
        send_open = new_send_open;

        debug!("Waiting for a connection request");
        let url = match recv_url.await {
            Ok(url) => url,
            Err(_) => {
                debug!("Frontend closed");
                return;
            }
        };

        let (send_connected, recv_connected) = oneshot::channel();
        let connecting = ConnectingClient { recv_connected };
        if let Err(_) = send_connecting.send(connecting) {
            debug!("Frontend closed");
            return;
        }

        debug!("Connecting to {}", url);
        let conn = match endpoint
            .connect(url)
            .await
            .map_err(WebTransportError::Connect)
        {
            Ok(conn) => conn,
            Err(err) => {
                let _ = send_connected.send(Err((err, recv_open)));
                return;
            }
        };

        debug!("Establishing channels");
        let channels_state = match shared::establish_channels::<C2S, S2C, C, false>(&conn).await {
            Ok(state) => state,
            Err(err) => {
                let _ = send_connected.send(Err(err));
                return;
            }
        };

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
        if let Err(_) = send_connected.send(Ok(connected)) {
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
}
