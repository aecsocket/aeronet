use std::future::Future;

use aeronet::client::ClientState;
use futures::channel::oneshot;

use super::{backend, ClientConfig, ClientError, Connected, Connecting, WebTransportClient};

type State = ClientState<Connecting, Connected>;

impl WebTransportClient {
    pub fn disconnected() -> Self {
        Self {
            state: State::Disconnected,
        }
    }

    pub fn connect_new(config: ClientConfig) -> (Self, impl Future<Output = ()> + Send) {
        let (send_connected, recv_connected) = oneshot::channel::<Connected>();
        let (send_err, recv_err) = oneshot::channel::<ClientError>();

        let frontend = Self {
            state: State::Connecting(Connecting {
                recv_connected,
                recv_err,
            }),
        };
        let backend = async move {
            if let Err(err) = backend::start(config, send_connected).await {
                let _ = send_err.send(err);
            }
        };
        (frontend, backend)
    }
}
