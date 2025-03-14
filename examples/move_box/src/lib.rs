#![doc = include_str!("../README.md")]

use {
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

impl Plugin for MoveBoxPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .enable_state_scoped_entities::<GameState>()
            .replicate::<Player>()
            .replicate::<PlayerPosition>()
            .replicate::<PlayerColor>()
            .add_client_event::<PlayerInput>(ChannelKind::Unreliable)
            .add_systems(
                FixedUpdate,
                (recv_input, apply_movement)
                    .chain()
                    .run_if(server_or_singleplayer),
            );
    }
}

/// Marker component for a player in the game.
#[derive(Debug, Clone, Component, Serialize, Deserialize)]
#[require(StateScoped<GameState>(|| StateScoped(GameState::Playing)))]
pub struct Player;

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
    mut players: Query<&mut PlayerInput>,
) {
    for &FromClient {
        client_entity,
        event: ref new_input,
    } in inputs.read()
    {
        let Ok(mut input) = players.get_mut(client_entity) else {
            continue;
        };
        *input = new_input.clone();
    }
}

fn apply_movement(time: Res<Time>, mut players: Query<(&PlayerInput, &mut PlayerPosition)>) {
    for (input, mut position) in &mut players {
        // make sure to validate inputs and normalize on the authority (server) side,
        // since we're accepting arbitrary client input
        if let Some(movement) = input.movement.try_normalize() {
            // only change `position` if we actually have a movement vector to apply
            // this saves bandwidth; we don't replicate position if we don't change it
            **position += movement * time.delta_secs() * MOVE_SPEED;
        }
    }
}
