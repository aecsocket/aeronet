use std::time::Duration;

use aeronet::{ClientTransportPlugin, ServerTransportPlugin, TryFromBytes, TryIntoBytes};
use aeronet_channel::{ChannelTransportClient, ChannelTransportServer};
use anyhow::Result;
use bevy::{app::ScheduleRunnerPlugin, prelude::*};

// config

#[derive(Debug, Clone)]
pub struct AppMessage(pub String);

impl TryIntoBytes for AppMessage {
    fn try_into_bytes(self) -> Result<Vec<u8>> {
        Ok(self.0.into_bytes())
    }
}

impl TryFromBytes for AppMessage {
    fn try_from_bytes(payload: &[u8]) -> Result<Self> {
        String::from_utf8(payload.to_owned().into_iter().collect())
            .map(|s| AppMessage(s))
            .map_err(|err| err.into())
    }
}

type Client = ChannelTransportClient<AppMessage, AppMessage>;

type Server = ChannelTransportServer<AppMessage, AppMessage>;

// logic

fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))),
            ClientTransportPlugin::<_, _, Client>::default(),
            ServerTransportPlugin::<_, _, Server>::default(),
        ))
        .run();
}
