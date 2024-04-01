use std::{fmt::Debug, future::Future, marker::PhantomData, net::SocketAddr};

use aeronet::{
    message::{TryFromBytes, TryIntoBytes},
    protocol::{ProtocolVersion, TransportProtocol},
};
use aeronet_proto::seq::Seq;
use derivative::Derivative;
use futures::channel::oneshot;
use steamworks::{ClientManager, SteamId};

pub mod backend;

#[derive(Derivative, thiserror::Error)]
#[derivative(
    Debug(
        bound = "<P::C2S as TryIntoBytes>::Error: Debug, <P::S2C as TryFromBytes>::Error: Debug"
    ),
    Clone(
        bound = "<P::C2S as TryIntoBytes>::Error: Clone, <P::S2C as TryFromBytes>::Error: Clone"
    )
)]
pub enum Error<P>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes,
    P::S2C: TryFromBytes,
{
    #[error("already connected")]
    AlreadyConnected,
    #[error("already disconnected")]
    AlreadyDisconnected,

    #[error("failed to convert message into bytes")]
    IntoBytes(#[source] <P::C2S as TryIntoBytes>::Error),
    #[error("failed to create message from bytes")]
    FromBytes(#[source] <P::S2C as TryFromBytes>::Error),
    #[error(transparent)]
    Backend(#[from] backend::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientMessageKey {
    msg_seq: Seq,
}

/// Identifier of a peer which a Steam client wants to connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectTarget {
    /// Peer identified by its IP address.
    Ip(SocketAddr),
    /// Peer identified by its Steam ID.
    Peer {
        /// Steam ID of the peer.
        id: SteamId,
        /// Port to connect on.
        virtual_port: i32,
    },
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
pub struct SteamClientTransport<P, M = ClientManager> {
    inner: Inner,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<(P, M)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportConfig {
    pub version: ProtocolVersion,
    pub recv_batch_size: usize,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
enum Inner {
    #[derivative(Default)]
    Disconnected,
    Connecting {
        recv_err: oneshot::Receiver<backend::Error>,
    },
}

impl<P, M> SteamClientTransport<P, M>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes,
    P::S2C: TryFromBytes,
    M: steamworks::Manager + Send + Sync + 'static,
{
    pub fn disconnected() -> Self {
        Self {
            inner: Inner::Disconnected,
            _phantom: PhantomData,
        }
    }

    pub fn connect_new(
        steam: steamworks::Client<M>,
        target: ConnectTarget,
        config: TransportConfig,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let (send_err, recv_err) = oneshot::channel();
        let (send_negotiating, recv_negotiating) = oneshot::channel();
        let backend = async move {
            if let Err(err) = backend::open(
                steam,
                target,
                config.version,
                config.recv_batch_size,
                send_negotiating,
            )
            .await
            {
                let _ = send_err.send(err);
            }
        };

        (
            Self {
                inner: Inner::Connecting { recv_err },
                _phantom: PhantomData,
            },
            backend,
        )
    }

    pub fn connect(
        &mut self,
        steam: steamworks::Client<M>,
        target: ConnectTarget,
        config: TransportConfig,
    ) -> Result<impl Future<Output = ()> + Send, Error<P>> {
        match self.inner {
            Inner::Disconnected => {
                let (this, backend) = Self::connect_new(steam, target, config);
                *self = this;
                Ok(backend)
            }
            Inner::Connecting { .. } => Err(Error::AlreadyConnected),
        }
    }
}
