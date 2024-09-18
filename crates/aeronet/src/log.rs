//! See [`SessionLogPlugin`].

use std::any::type_name;

use bevy_app::prelude::*;
use bevy_core::Name;
use bevy_ecs::prelude::*;
use tracing::{info, warn};

use crate::session::{Connected, DisconnectReason, Disconnected, Session};

/// Uses [`tracing`] to log [session]-related events.
///
/// This will emit:
/// - a connecting message when [`Session`] is added to an session
/// - a connected message when [`Connected`] is added to an session
/// - a disconnected message when [`Disconected`] is triggered on a session
///
/// This is included by default as part of [`AeronetPlugins`]. If you do not
/// want the log messages, you can disable the plugin via
/// [`PluginGroupBuilder::disable`]:
///
/// ```
/// use bevy_app::prelude::*;
/// use aeronet::{AeronetPlugins, log::SessionLogPlugin};
///
/// # fn run(app: &mut App) {
/// app.add_plugins(AeronetPlugins.build().disable::<SessionLogPlugin>());
/// # }
/// ```
///
/// [`PluginGroupBuilder::disable`]: bevy_app::PluginGroupBuilder::disable
#[derive(Debug)]
pub struct SessionLogPlugin;

impl Plugin for SessionLogPlugin {
    fn build(&self, app: &mut App) {
        app.observe(on_connecting)
            .observe(on_connected)
            .observe(on_disconnected);
    }
}

fn on_connecting(trigger: Trigger<OnAdd, Session>, names: Query<Option<&Name>>) {
    let session = trigger.entity();
    let name = names.get(session).unwrap_or_else(|_| {
        panic!(
            "entity {session} should exist because we are adding `{}` to it",
            type_name::<Session>()
        )
    });

    let display_name = display_name(session, name);
    info!("Session {display_name} connecting");
}

fn on_connected(trigger: Trigger<OnAdd, Connected>, names: Query<Option<&Name>>) {
    let session = trigger.entity();
    let name = names.get(session).unwrap_or_else(|_| {
        panic!(
            "entity {session} should exist because we are adding `{}` to it",
            type_name::<Connected>()
        )
    });

    let display_name = display_name(session, name);
    info!("Session {display_name} connected");
}

fn on_disconnected(trigger: Trigger<Disconnected>, names: Query<Option<&Name>>) {
    let session = trigger.entity();
    let name = names.get(session).unwrap_or_else(|_| {
        panic!(
            "`{}` should not be triggered with entity {session} that doesn't exist",
            type_name::<Disconnected>()
        )
    });

    let display_name = display_name(session, name);
    match &**trigger.event() {
        DisconnectReason::User(reason) => {
            info!("Session {display_name} disconnected by user: {reason}");
        }
        DisconnectReason::Peer(reason) => {
            info!("Session {display_name} disconnected by peer: {reason}");
        }
        DisconnectReason::Error(err) => {
            warn!("Session {display_name} disconnected due to error: {err:#}");
        }
    }
}

fn display_name(entity: Entity, name: Option<&Name>) -> String {
    if let Some(name) = name {
        format!("\"{name}\" ({entity})")
    } else {
        format!("{entity}")
    }
}
