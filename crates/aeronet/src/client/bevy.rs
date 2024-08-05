//! Client-side Bevy types and items for networking.
//!
//! Note that this crate itself does not provide any logic for using transports
//! i.e. polling, connecting, or writing. This module simply provides the items
//! so that other crates can build on top of them, allowing interoperability.

use std::{fmt::Debug, marker::PhantomData};

use bevy_ecs::prelude::*;
use derivative::Derivative;

use super::{ClientTransport, DisconnectReason};

/// System set for client-side networking systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ClientTransportSet {
    /// Handles receiving data from the transport and updating its internal
    /// state using [`ClientTransport::poll`].
    Recv,
    /// Handles flushing buffered messages and sending out buffered data using
    /// [`ClientTransport::flush`].
    Send,
}

/// A [`Condition`]-satisfying system that returns `true` if the client `T`
/// exists *and* is in the [`Connected`] state.
///
/// # Example
///
/// ```
/// # use bevy_app::prelude::*;
/// # use bevy_ecs::prelude::*;
/// # use aeronet::client::{ClientTransport, client_connected};
/// # fn run<T: ClientTransport + Resource>() {
/// let mut app = App::new();
/// app.add_systems(Update, my_system::<T>.run_if(client_connected::<T>));
///
/// fn my_system<T: ClientTransport + Resource>(client: Res<T>) {
///     // ..
/// }
/// # }
/// ```
///
/// [`Condition`]: bevy_ecs::schedule::Condition
/// [`Connected`]: crate::client::ClientState::Connected
#[must_use]
pub fn client_connected<T: ClientTransport + Resource>(client: Option<Res<T>>) -> bool {
    client.is_some_and(|client| client.state().is_connected())
}

/// A [`Condition`]-satisfying system that returns `true` if the client `T` does
/// not exist *or* is in the [`Disconnected`] state.
///
/// # Example
///
/// ```
/// # use bevy_app::prelude::*;
/// # use bevy_ecs::prelude::*;
/// # use aeronet::client::{ClientTransport, client_disconnected};
/// # fn run<T: ClientTransport + Resource>() {
/// let mut app = App::new();
/// app.add_systems(Update, my_system::<T>.run_if(client_disconnected::<T>));
///
/// fn my_system<T: ClientTransport + Resource>(client: Res<T>) {
///     // ..
/// }
/// # }
/// ```
///
/// [`Condition`]: bevy_ecs::schedule::Condition
/// [`Disconnected`]: crate::client::ClientState::Disconnected
#[must_use]
pub fn client_disconnected<T: ClientTransport + Resource>(client: Option<Res<T>>) -> bool {
    client.map_or(true, |client| client.state().is_disconnected())
}

/// The client has fully established a connection to the server,
/// changing state to [`ClientState::Connected`].
///
/// After this event, you can run your game initialization logic such as
/// receiving the initial world state and e.g. showing a spawn screen.
///
/// See [`ClientEvent::Connected`].
///
/// [`ClientState::Connected`]: crate::client::ClientState::Connected
/// [`ClientEvent::Connected`]: crate::client::ClientEvent::Connected
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""), Default(bound = ""))]
pub struct LocalClientConnected<T: ClientTransport> {
    #[derivative(Debug = "ignore")]
    #[doc(hidden)]
    pub _phantom: PhantomData<T>,
}

/// The client has unrecoverably lost connection from its previously
/// connected server changing state to [`ClientState::Disconnected`].
///
/// This is emitted for *any* reason that the client may be disconnected,
/// including user code calling [`ClientTransport::disconnect`], therefore
/// this may be used as a signal to tear down the app state.
///
/// See [`ClientEvent::Disconnected`].
///
/// [`ClientState::Disconnected`]: crate::client::ClientState::Disconnected
/// [`ClientEvent::Disconnected`]: crate::client::ClientEvent::Disconnected
#[derive(Derivative, Event)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub struct LocalClientDisconnected<T: ClientTransport> {
    /// Why the client lost connection.
    pub reason: DisconnectReason<T::Error>,
}

/// The peer acknowledged that they have fully received a message sent by
/// us.
///
/// See [`ClientEvent::Ack`].
///
/// [`ClientEvent::Ack`]: crate::client::ClientEvent::Ack
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct AckFromServer<T: ClientTransport> {
    /// Key of the sent message, obtained by [`ClientTransport::send`].
    pub msg_key: T::MessageKey,
}

/// Our client believes that an unreliable message has probably been lost in
/// transit.
///
/// An implementation is allowed to not emit this event if it is not able to.
///
/// See [`ClientEvent::Nack`].
///
/// [`ClientEvent::Nack`]: crate::client::ClientEvent::Nack
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct NackFromServer<T: ClientTransport> {
    /// Key of the sent message, obtained by [`ClientTransport::send`].
    pub msg_key: T::MessageKey,
}
