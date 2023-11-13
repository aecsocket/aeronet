use aeronet::{TryIntoBytes, TryFromBytes, Message};

use crate::EndpointInfo;

pub mod back;
pub mod front;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientKey(usize);

impl ClientKey {
    pub fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> usize {
        self.0
    }
}

type WebTransportError<C2S, S2C> = crate::WebTransportError<S2C, C2S>;
type ChannelError<C2S, S2C> = crate::ChannelError<S2C, C2S>;

const CHANNEL_CAP: usize = 128;
const DATA_CAP: usize = 65536;

#[derive(Debug)]
pub enum Signal<C2S, S2C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes,
{
    Incoming {
        client: ClientKey,
    },
    Accepted {
        client: ClientKey,
        authority: String,
        path: String,
        origin: Option<String>,
        user_agent: Option<String>,
    },
    Connected {
        client: ClientKey,
    },
    UpdateEndpointInfo {
        client: ClientKey,
        info: EndpointInfo,
    },
    Recv {
        from: ClientKey,
        msg: C2S,
    },
    Disconnected {
        client: ClientKey,
        reason: WebTransportError<C2S, S2C>,
    },
}

#[derive(Debug, Clone)]
enum Request<S2C> {
    Send { to: ClientKey, msg: S2C },
    Disconnect { target: ClientKey },
}
