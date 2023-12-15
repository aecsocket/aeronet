use aeronet::{ChannelProtocol, OnChannel, TryAsBytes, TryFromBytes, TransportClient, ClientEvent};
use derivative::Derivative;

use crate::{bindings::WebTransport, WebTransportError, EndpointInfo};

/// Implementation of [`TransportClient`] using the WebTransport protocol.
///
/// See the [crate-level docs](crate).
#[derive(Debug, Derivative)]
#[derivative(Default)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct WebTransportClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    state: State<P>,
}

#[derive(Debug, Default)]
enum State<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    #[default]
    Disconnected,
    Connecting,
    _X(Vec<P>),
}

impl<P> WebTransportClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    #[must_use]
    pub fn closed() -> Self {
        Self {
            state: State::Disconnected,
        }
    }

    pub fn connecting(url: impl AsRef<str>) -> Result<Self, WebTransportError<P>> {
        let url = url.as_ref();
        let transport = WebTransport::new(url)
            .map_err(|_| WebTransportError::CreateClient)?;

        todo!()
    }
}

impl<P> TransportClient<P> for WebTransportClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    type Error = WebTransportError<P>;

    type ConnectionInfo = EndpointInfo;

    type Event = ClientEvent<P, WebTransportClient<P>>;

    fn connection_info(&self) -> Option<Self::ConnectionInfo> {
        match self.state {
            State::Disconnected => None,
        }
    }
}
