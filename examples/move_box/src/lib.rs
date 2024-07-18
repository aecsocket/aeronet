#![doc = include_str!("../README.md")]

use aeronet_webtransport::proto::session::SessionConfig;
use bevy::prelude::*;
use bevy_replicon::prelude::*;
use serde::{Deserialize, Serialize};

const MOVE_SPEED: f32 = 300.0;

const MAX_RECV_MEMORY_USAGE: usize = 1024 * 1024 * 4;

/// Sets up replication and basic game systems.
#[derive(Debug)]
pub struct MoveBoxPlugin;

impl Plugin for MoveBoxPlugin {
    fn build(&self, app: &mut App) {
        app.replicate::<PlayerPosition>()
            .replicate::<PlayerColor>()
            .add_client_event::<MoveDirection>(ChannelKind::Ordered)
            .add_systems(Update, apply_movement.run_if(has_authority));
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
pub struct MoveDirection(pub Vec2);

/// Creates the base [`SessionConfig`] with no lanes registered.
pub fn base_session_config() -> SessionConfig {
    SessionConfig::new(MAX_RECV_MEMORY_USAGE)
}

fn apply_movement(
    time: Res<Time>,
    mut move_events: EventReader<FromClient<MoveDirection>>,
    mut players: Query<(&Player, &mut PlayerPosition)>,
) {
    for FromClient { client_id, event } in move_events.read() {
        for (player, mut position) in &mut players {
            if *client_id == player.0 {
                **position += event.0 * time.delta_seconds() * MOVE_SPEED;
            }
        }
    }
}
