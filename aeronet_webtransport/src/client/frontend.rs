use std::future::Future;

use aeronet::client::ClientState;
use futures::channel::oneshot;
use xwt_core::utils::maybe;

use super::{backend, ClientConfig, ClientError, Connected, Connecting, WebTransportClient};

type State = ClientState<Connecting, Connected>;

impl WebTransportClient {
    pub fn disconnected() -> Self {
        Self {
            state: State::Disconnected,
        }
    }

    pub fn disconnect(&mut self) -> Result<(), ClientError> {
        match self.state {
            State::Disconnected => Err(ClientError::AlreadyDisconnected),
            State::Connecting(_) | State::Connected(_) => {
                *self = Self::disconnected();
                Ok(())
            }
        }
    }

    pub fn connect_new(
        config: ClientConfig,
        target: impl Into<String>,
    ) -> (Self, impl Future<Output = ()> + maybe::Send) {
        let (send_connected, recv_connected) = oneshot::channel::<Connected>();
        let (send_err, recv_err) = oneshot::channel::<ClientError>();

        let frontend = Self {
            state: State::Connecting(Connecting {
                recv_connected,
                recv_err,
            }),
        };
        let target = target.into();
        let backend = async move {
            if let Err(err) = backend::start(config, target, send_connected).await {
                let _ = send_err.send(err);
            }
        };
        (frontend, backend)
    }

    pub fn connect(
        &mut self,
        config: ClientConfig,
        target: impl Into<String>,
    ) -> Result<impl Future<Output = ()> + maybe::Send, ClientError> {
        match self.state {
            State::Disconnected => {
                let (frontend, backend) = Self::connect_new(config, target);
                *self = frontend;
                Ok(backend)
            }
            State::Connecting(_) | State::Connected(_) => Err(ClientError::AlreadyConnected),
        }
    }
}
