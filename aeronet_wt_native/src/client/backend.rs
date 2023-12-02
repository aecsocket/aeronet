use aeronet::{Message, TryFromBytes, OnChannel, TryIntoBytes, ChannelKey};
use tokio::sync::oneshot;
use tracing::debug;
use wtransport::{ClientConfig, Endpoint};

use super::{WebTransportError, ConnectedResult};

pub(super) async fn start<C2S, S2C, C>(
    config: ClientConfig,
    send_connected: oneshot::Sender<ConnectedResult<C2S, S2C, C>>,
) where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    debug!("Starting backend");
    let endpoint = match Endpoint::client(config).map_err(WebTransportError::CreateEndpoint) {
        Ok(endpoint) => endpoint,
        Err(err) => {
            let _ = send_connected.send(Err(err));
            return;
        }
    };
    debug!("Created endpoint");
}
