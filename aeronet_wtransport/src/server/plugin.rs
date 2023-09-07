use bevy::prelude::*;
use tokio::runtime::Runtime;
use wtransport::ServerConfig;

use crate::server::SyncServer;

#[derive(Debug, Resource)]
pub struct WebTransportServer(SyncServer);

impl WebTransportServer {
    pub fn new(config: ServerConfig, rt: &Runtime) -> Self {
        let (sync_server, async_server) = crate::server::create(config);

        rt.spawn(async move {
            // todo
            async_server.listen();
        });

        Self(sync_server)
    }
}
