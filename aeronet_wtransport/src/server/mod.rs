#![warn(clippy::future_not_send)]

mod end;
#[cfg(feature = "bevy")]
pub mod plugin;

pub use end::{create, WtServerBackend, WtServerFrontend};

use std::{collections::HashMap, fmt::Display};

use wtransport::error::{ConnectionError, StreamOpeningError};

use crate::{StreamId, StreamKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Reflect))]
pub struct ClientId(usize);

impl Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ClientId {
    pub fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerStream {
    Datagram,
    Bi(StreamId),
    S2C(StreamId),
}

impl ServerStream {
    pub fn as_kind(self) -> StreamKind {
        match self {
            Self::Datagram => StreamKind::Datagram,
            Self::Bi(id) => StreamKind::Bi(id),
            Self::S2C(id) => StreamKind::S2C(id),
        }
    }
}

#[derive(Debug)]
pub enum B2F<C2S> {
    Started,
    Incoming {
        client: ClientId,
        authority: String,
        path: String,
        headers: HashMap<String, String>,
    },
    Connected {
        client: ClientId,
    },
    Recv {
        client: ClientId,
        msg: C2S,
    },
    Disconnected {
        client: ClientId,
        reason: SessionError,
    },
}

#[derive(Debug, Clone)]
pub enum F2B<S2C> {
    Send {
        client: ClientId,
        stream: ServerStream,
        msg: S2C,
    },
    Disconnect {
        client: ClientId,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("server closed")]
    ServerClosed,
    #[error("forced disconnect by server")]
    ForceDisconnect,
    #[error("failed to receive incoming session")]
    RecvSession(#[source] ConnectionError),
    #[error("failed to accept session")]
    AcceptSession(#[source] ConnectionError),
    #[error("on {stream:?}")]
    Stream {
        stream: StreamKind,
        #[source]
        source: StreamError,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("failed to connect bi/S2C")]
    Connect(#[source] ConnectionError),
    #[error("failed to open")]
    Open(#[source] StreamOpeningError),
    #[error("failed to accept C2S")]
    Accept(#[source] ConnectionError),
    #[error("failed to receive data")]
    Recv(#[source] anyhow::Error),
    #[error("failed to deserialize incoming data")]
    Deserialize(#[source] anyhow::Error),
    #[error("failed to send data")]
    Send(#[source] anyhow::Error),
    #[error("failed to serialize outgoing data")]
    Serialize(anyhow::Error),
    #[error("closed by client")]
    Closed,
}
