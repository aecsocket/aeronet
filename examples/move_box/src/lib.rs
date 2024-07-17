#![doc = include_str!("../README.md")]

use bevy::prelude::*;
use bevy_replicon::core::ClientId;
use serde::{Deserialize, Serialize};

/// ID of the player this entity represents.
#[derive(Debug, Clone, Component, Serialize, Deserialize)]
pub struct Player(pub ClientId);

/// Player's box position.
#[derive(Debug, Clone, Component, Serialize, Deserialize)]
pub struct PlayerPosition(pub Vec2);

/// Player's box color.
#[derive(Debug, Clone, Component, Serialize, Deserialize)]
pub struct PlayerColor(pub Color);

/// Player sent an input to move their box.
#[derive(Debug, Clone, Event, Serialize, Deserialize)]
pub struct MoveDirection(pub Vec2);
