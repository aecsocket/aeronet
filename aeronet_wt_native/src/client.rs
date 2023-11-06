use std::io;

use derivative::Derivative;
use tokio::sync::oneshot;
use wtransport::{endpoint::endpoint_side::Client, ClientConfig, Endpoint};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("failed to create client endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("backend closed")]
    BackendClosed,
}

// a worse version of std's Poll lol
pub enum Transition<T, R> {
    Pending(T),
    Ready(R),
}

use Transition::{Ready, Pending};

//

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Creating {
    #[derivative(Debug = "ignore")]
    recv_result: oneshot::Receiver<Result<Disconnected, ClientError>>,
}

impl Creating {
    pub fn new(config: ClientConfig) -> (Self, ClientBackend) {
        let (send_result, recv_result) = oneshot::channel::<Result<_, _>>();
        (
            Self { recv_result },
            ClientBackend {
                config,
                send_result,
            },
        )
    }

    pub fn poll(mut self) -> Transition<Self, Result<Disconnected, ClientError>> {
        match self.recv_result.try_recv() {
            Ok(result) => Ready(result),
            Err(oneshot::error::TryRecvError::Empty) => Pending(self),
            Err(oneshot::error::TryRecvError::Closed) => Ready(Err(ClientError::BackendClosed)),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct ClientBackend {
    #[derivative(Debug = "ignore")]
    config: ClientConfig,
    #[derivative(Debug = "ignore")]
    send_result: oneshot::Sender<Result<Disconnected, ClientError>>,
}

impl ClientBackend {
    pub async fn start(self) {
        tokio::spawn(listen(self.config, self.send_result));
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
struct ConnectRequest {
    url: String,
    #[derivative(Debug = "ignore")]
    send_connected: oneshot::Sender<()>,
}

async fn listen(
    config: ClientConfig,
    send_result: oneshot::Sender<Result<Disconnected, ClientError>>,
) {
    let endpoint = match create_endpoint(config).await {
        Ok(endpoint) => endpoint,
        Err(err) => {
            let _ = send_result.send(Err(err));
            return;
        }
    };

    let (send_req, recv_req) = oneshot::channel::<ConnectRequest>();
    let _ = send_result.send(Ok(Disconnected { send_req }));
    listen_connect(endpoint, recv_req).await;
}

async fn create_endpoint(config: ClientConfig) -> Result<Endpoint<Client>, ClientError> {
    Endpoint::client(config).map_err(ClientError::CreateEndpoint)
}

//

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Disconnected {
    #[derivative(Debug = "ignore")]
    send_req: oneshot::Sender<ConnectRequest>,
}

impl Disconnected {
    pub fn connect(self, url: impl Into<String>) -> Result<Connecting, ClientError> {
        let (send_connected, recv_connected) = oneshot::channel::<()>();
        let req = ConnectRequest {
            url: url.into(),
            send_connected,
        };

        match self.send_req.send(req) {
            Ok(_) => Ok(Connecting { recv_connected }),
            Err(_) => Err(ClientError::BackendClosed),
        }
    }
}

async fn listen_connect(endpoint: Endpoint<Client>, recv_req: oneshot::Receiver<ConnectRequest>) {
    let Ok(req) = recv_req.await else {
        return;
    };

    endpoint.connect(req.url).await;
}

//

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Connecting {
    #[derivative(Debug = "ignore")]
    recv_connected: oneshot::Receiver<()>,
}

impl Connecting {
    pub fn poll(mut self) -> Transition<Self, Result<Connected, ClientError>> {
        match self.recv_connected.try_recv() {
            Ok(_) => Ready(Ok(Connected)),
            Err(oneshot::error::TryRecvError::Empty) => Pending(self),
            Err(oneshot::error::TryRecvError::Closed) => Ready(Err(ClientError::BackendClosed)),
        }
    }
}

//

#[derive(Debug)]
pub struct Connected;

//

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum WebTransportClient {
    Creating(Creating),
    Disconnected(Disconnected),
    Connecting(Connecting),
    Connected(Connected),
}

impl From<Creating> for WebTransportClient {
    fn from(value: Creating) -> Self {
        Self::Creating(value)
    }
}

impl From<Disconnected> for WebTransportClient {
    fn from(value: Disconnected) -> Self {
        Self::Disconnected(value)
    }
}

impl From<Connecting> for WebTransportClient {
    fn from(value: Connecting) -> Self {
        Self::Connecting(value)
    }
}

impl From<Connected> for WebTransportClient {
    fn from(value: Connected) -> Self {
        Self::Connected(value)
    }
}
