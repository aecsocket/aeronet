#![doc = include_str!("../README.md")]

use {
    aeronet_replicon::channel::IntoLanes,
    aeronet_webtransport::{proto::session::SessionConfig, runtime::WebTransportRuntime},
    bevy::prelude::*,
    bevy_replicon::prelude::*,
    serde::{Deserialize, Serialize},
};

/// How many units a player may move in a single second.
const MOVE_SPEED: f32 = 250.0;

/// How many times per second we will replicate entity components.
pub const TICK_RATE: u16 = 128;

/// Sets up replication and basic game systems.
#[derive(Debug)]
pub struct MoveBoxPlugin;

/// Whether the game is currently being simulated or not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, States)]
pub enum GameState {
    /// Game is not being simulated.
    #[default]
    None,
    /// Game is being simulated.
    Playing,
}

/// Creates a [`SessionConfig`] from [`RepliconChannels`], customized for this
/// app.
///
/// Both the client and server should have the same [`SessionConfig`].
#[must_use]
pub fn session_config(channels: &RepliconChannels) -> SessionConfig {
    SessionConfig::default()
        .with_client_lanes(channels.client_channels().into_lanes())
        .with_server_lanes(channels.server_channels().into_lanes())
}

impl Plugin for MoveBoxPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .enable_state_scoped_entities::<GameState>()
            // use the convenience resource WebTransportRuntime for spawning tasks
            // platform-independently (native using tokio, or WASM using wasm-bindgen-futures)
            .init_resource::<WebTransportRuntime>()
            .replicate::<Player>()
            .replicate::<PlayerPosition>()
            .replicate::<PlayerColor>()
            .add_client_event::<PlayerInput>(ChannelKind::Ordered)
            .add_systems(FixedUpdate, (recv_input, apply_movement).chain().run_if(has_authority));
    }
}

/// Marker component for a player in the game.
#[derive(Debug, Clone, Component, Serialize, Deserialize)]
pub struct Player;

/// Player who is being controlled by a specific [`ClientId`] connected to our
/// server.
#[derive(Debug, Clone, Component, Deref, DerefMut, Serialize, Deserialize)]
pub struct ClientPlayer(pub ClientId);

/// Player's box position.
#[derive(Debug, Clone, Component, Deref, DerefMut, Serialize, Deserialize)]
pub struct PlayerPosition(pub Vec2);

/// Player's box color.
#[derive(Debug, Clone, Component, Deref, DerefMut, Serialize, Deserialize)]
pub struct PlayerColor(pub Color);

/// Player's inputs that they send to control their box.
#[derive(Debug, Clone, Default, Event, Serialize, Deserialize)]
pub struct PlayerInput {
    /// Lateral movement vector.
    ///
    /// The client has full control over this field, and may send an
    /// unnormalized vector! Authorities must ensure that they normalize or
    /// zero this vector before using it for movement updates.
    pub movement: Vec2,
}

fn recv_input(
    mut inputs: EventReader<FromClient<PlayerInput>>,
    mut players: Query<(&ClientPlayer, &mut PlayerInput)>,
) {
    for &FromClient {
        client_id,
        event: ref new_input,
    } in inputs.read()
    {
        for (player, mut old_input) in &mut players {
            if client_id == **player {
                *old_input = new_input.clone();
            }
        }
    }
}

fn apply_movement(time: Res<Time>, mut players: Query<(&PlayerInput, &mut PlayerPosition)>) {
    for (input, mut position) in &mut players {
        // make sure to validate inputs and normalize on the server side,
        // since we're accepting arbitrary client input
        if let Some(movement) = input.movement.try_normalize() {
            // only change `position` if we actually have a movement vector to apply
            // this saves bandwidth; we don't replicate position if we don't change it
            **position += movement * time.delta_seconds() * MOVE_SPEED;
        }
    }
}
