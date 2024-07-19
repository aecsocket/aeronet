#![doc = include_str!("../README.md")]

use std::future::Future;

use aeronet_webtransport::proto::session::SessionConfig;
use bevy::prelude::*;
use bevy_replicon::prelude::*;
use serde::{Deserialize, Serialize};

const MOVE_SPEED: f32 = 2500.0;

const MAX_MEMORY_USAGE: usize = 1024 * 1024 * 4;

/// Sets up replication and basic game systems.
#[derive(Debug)]
pub struct MoveBoxPlugin;

impl Plugin for MoveBoxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AsyncRuntime>()
            .add_plugins(RepliconPlugins.build().set(ServerPlugin {
                tick_policy: TickPolicy::MaxTickRate(128),
                ..Default::default()
            }))
            .replicate::<PlayerPosition>()
            .replicate::<PlayerColor>()
            .add_client_event::<PlayerMove>(ChannelKind::Ordered)
            .add_systems(Update, apply_movement.run_if(has_authority));
    }
}

/// Platform-agnostic async task runtime.
#[derive(Debug, Deref, DerefMut, Resource)]
pub struct AsyncRuntime {
    #[cfg(target_family = "wasm")]
    _priv: (),
    #[cfg(not(target_family = "wasm"))]
    runtime: tokio::runtime::Runtime,
}

impl FromWorld for AsyncRuntime {
    fn from_world(_: &mut World) -> Self {
        #[cfg(target_family = "wasm")]
        {
            Self { _priv: () }
        }
        #[cfg(not(target_family = "wasm"))]
        {
            Self {
                runtime: tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .unwrap(),
            }
        }
    }
}

impl AsyncRuntime {
    /// Spawns a future on this runtime.
    pub fn spawn<F>(&self, future: F)
    where
        F: Future + Send + 'static,
        F::Output: Send,
    {
        #[cfg(target_family = "wasm")]
        {
            wasm_bindgen_futures::spawn_local(future);
        }
        #[cfg(not(target_family = "wasm"))]
        {
            self.runtime.spawn(future);
        }
    }
}

/// ID of the player this entity represents.
#[derive(Debug, Clone, Component, Serialize, Deserialize)]
pub struct Player(pub ClientId);

/// Player's box position.
#[derive(Debug, Clone, Component, Deref, DerefMut, Serialize, Deserialize)]
pub struct PlayerPosition(pub Vec2);

/// Player's box color.
#[derive(Debug, Clone, Component, Deref, DerefMut, Serialize, Deserialize)]
pub struct PlayerColor(pub Color);

/// Player sent an input to move their box.
#[derive(Debug, Clone, Event, Serialize, Deserialize)]
pub struct PlayerMove(pub Vec2);

/// Creates the base [`SessionConfig`] with no lanes registered.
pub fn base_session_config() -> SessionConfig {
    SessionConfig::new(MAX_MEMORY_USAGE)
}

fn apply_movement(
    time: Res<Time>,
    mut move_events: EventReader<FromClient<PlayerMove>>,
    mut players: Query<(&Player, &mut PlayerPosition)>,
) {
    for FromClient {
        client_id,
        event: PlayerMove(delta),
    } in move_events.read()
    {
        for (player, mut position) in &mut players {
            if *client_id == player.0 {
                **position += delta.normalize_or_zero() * time.delta_seconds() * MOVE_SPEED;
            }
        }
    }
}
