use aeronet::{ChannelProtocol, OnChannel, TryAsBytes, TryFromBytes, TransportClient, ClientEvent};
use derivative::Derivative;
use futures::channel::oneshot;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::DomException;

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
    Connecting(ConnectingClient<P>),
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
        todo!()
    }
}

fn err_msg(js: JsValue) -> String {
    match js.dyn_ref::<DomException>() {
        Some(err) => err.message(),
        None => "<unknown>".to_owned(),
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
struct ConnectingClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    #[derivative(Debug = "ignore")]
    recv_connected: oneshot::Receiver<ConnectedClientResult<P>>
}

impl<P> ConnectingClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    fn new(url: impl AsRef<str>) -> Result<Self, WebTransportError<P>> {
        let url = url.as_ref();
        let transport = WebTransport::new(url)
            .map_err(|err| WebTransportError::CreateClient(err_msg(err)))?;

        let (send_connected, recv_connected) = oneshot::channel();
        Ok(Self {
            state: State::Connecting(ConnectingClient { recv_connected }),
        })
    }
}

struct ConnectedClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{}

type ConnectedClientResult<P> = Result<ConnectedClient<P>, WebTransportError<P>>;

// impl<P> TransportClient<P> for WebTransportClient<P>
// where
//     P: ChannelProtocol,
//     P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
//     P::S2C: TryFromBytes,
// {
//     type Error = WebTransportError<P>;

//     type ConnectionInfo = EndpointInfo;

//     type Event = ClientEvent<P, WebTransportClient<P>>;

//     fn connection_info(&self) -> Option<Self::ConnectionInfo> {
//         match self.state {
//             State::Disconnected => None,
//         }
//     }
// }
