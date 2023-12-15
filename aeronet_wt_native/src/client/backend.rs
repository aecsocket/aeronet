use aeronet::{ChannelProtocol, OnChannel, TryAsBytes, TryFromBytes};
use tokio::sync::{mpsc, oneshot};
use tracing::debug;
use wtransport::{endpoint::endpoint_side, ClientConfig, Connection, Endpoint};

use crate::{
    shared::{self, ChannelsState},
    EndpointInfo,
};

use super::{ConnectedClient, ConnectedClientResult, WebTransportError};

pub(super) async fn start<P>(
    config: ClientConfig,
    url: String,
    send_connected: oneshot::Sender<ConnectedClientResult<P>>,
) where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    let (endpoint, conn, channels) = match connect::<P>(config, url).await {
        Ok(t) => t,
        Err(err) => {
            debug!("Failed to connect");
            let _ = send_connected.send(Err(err));
            return;
        }
    };

    let (send_c2s, recv_c2s) = mpsc::unbounded_channel();
    let (send_s2c, recv_s2c) = mpsc::unbounded_channel();
    let (send_info, recv_info) = mpsc::unbounded_channel();
    let (send_err, recv_err) = oneshot::channel();
    let connected = ConnectedClient::<P> {
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
    if let Err(err) = shared::handle_connection::<P, P::C2S, P::S2C>(
        conn, channels, send_info, send_s2c, recv_c2s,
    )
    .await
    {
        debug!("Disconnected with error");
        let _ = send_err.send(err);
    } else {
        debug!("Disconnected without error");
    }
}

async fn connect<P>(
    config: ClientConfig,
    url: String,
) -> Result<
    (
        Endpoint<endpoint_side::Client>,
        Connection,
        ChannelsState<P, P::C2S, P::S2C>,
    ),
    WebTransportError<P>,
>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    debug!("Creating endpoint for {url}");
    let endpoint = Endpoint::client(config).map_err(WebTransportError::Endpoint)?;

    debug!("Connecting");
    let conn = endpoint
        .connect(url.clone())
        .await
        .map_err(WebTransportError::Connect)?;

    debug!("Establishing channels");
    let channels = shared::establish_channels::<P, P::C2S, P::S2C, false>(&conn).await?;

    Ok((endpoint, conn, channels))
}
