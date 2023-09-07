#[cfg(feature = "bevy")]
pub mod plugin;

use std::io;

use anyhow::Result;
use tokio::sync::mpsc;
use wtransport::{
    endpoint::IncomingSession,
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
pub struct SyncServer {
    recv_error: mpsc::Receiver<ServerError>,
    send_close: mpsc::Sender<()>,
}

impl Drop for SyncServer {
    fn drop(&mut self) {
        let _ = self.send_close.try_send(());
    }
}

pub struct AsyncServer {
    config: ServerConfig,
    recv_close: mpsc::Receiver<()>,
    chans: AsyncChannels,
}

#[derive(Debug, Clone)]
struct AsyncChannels {
    send_error: mpsc::Sender<ServerError>,
}

pub fn create(config: ServerConfig) -> (SyncServer, AsyncServer) {
    let (send_error, recv_error) = mpsc::channel::<ServerError>(BUFFER_SIZE);
    let (send_close, recv_close) = mpsc::channel::<()>(BUFFER_SIZE);

    let sync_chans: SyncServer = SyncServer {
        recv_error,
        send_close,
    };

    let async_chans = AsyncServer {
        config,
        recv_close,
        chans: AsyncChannels { send_error },
    };

    (sync_chans, async_chans)
}

impl AsyncServer {
    pub async fn listen(mut self) -> Result<(), ServerError> {
        let endpoint = Endpoint::server(self.config)
            .map_err(|err| ServerError::CreateEndpoint(err))?;

        loop {
            if self.recv_close.try_recv().is_ok() {
                break;
            }
    
            let session = endpoint.accept().await;
            let chans = self.chans.clone();
            tokio::spawn(async move {
                if let Err(err) = accept_session(session, chans.clone()).await {
                    let _ = chans.send_error.send(err);
                }
            });
        }

        Ok(())
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
