use std::io;

use anyhow::Result;
use tokio::{runtime::Runtime, sync::mpsc};
use wtransport::{
    endpoint::{IncomingSession, Server},
    Endpoint, ServerConfig, error::ConnectionError,
};

const BUFFER_SIZE: usize = 128;

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to receive incoming session")]
    RecvSession(#[source] ConnectionError),
    #[error("failed to accept session")]
    AcceptSession(#[source] ConnectionError),
}

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct WebTransportServer(SyncChannels);

impl Drop for WebTransportServer {
    fn drop(&mut self) {
        let _ = self.0.send_close.try_send(());
    }
}

#[derive(Debug)]
struct SyncChannels {
    recv_error: mpsc::Receiver<ServerError>,
    send_close: mpsc::Sender<()>,
}

#[derive(Debug, Clone)]
struct AsyncChannels {
    send_error: mpsc::Sender<ServerError>,
}

impl WebTransportServer {
    pub async fn new(config: ServerConfig) -> io::Result<Self> {
        // needs to be in a tokio runtime here
        let endpoint = Endpoint::server(config)?;

        let (send_error, recv_error) = mpsc::channel::<ServerError>(BUFFER_SIZE);
        let (send_close, recv_close) = mpsc::channel::<()>(BUFFER_SIZE);

        let sync_chans: SyncChannels = SyncChannels {
            recv_error,
            send_close,
        };

        let async_chans = AsyncChannels { send_error };

        tokio::spawn(async move {
            listen(endpoint, async_chans, recv_close).await;
        });

        Ok(Self(sync_chans))
    }
}

async fn listen(
    endpoint: Endpoint<Server>,
    chans: AsyncChannels,
    mut recv_close: mpsc::Receiver<()>,
) {
    loop {
        if recv_close.try_recv().is_ok() {
            break;
        }

        let session = endpoint.accept().await;
        let chans = chans.clone();
        tokio::spawn(async move {
            if let Err(err) = accept_session(session, chans.clone()).await {
                let _ = chans.send_error.send(err);
            }
        });
    }
}

async fn accept_session(session: IncomingSession, chans: AsyncChannels) -> Result<(), ServerError> {
    let conn = session.await
        .map_err(|err| ServerError::RecvSession(err))?;
    let conn = conn.accept().await
        .map_err(|err| ServerError::AcceptSession(err))?;
    
    let _ = conn.send_datagram("abcdef");

    Ok(())
}
