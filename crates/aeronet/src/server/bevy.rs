//! Server-side Bevy types and items for networking.
//!
//! Note that this crate itself does not provide any logic for using transports
//! i.e. polling, connecting, or writing. This module simply provides the items
//! so that other crates can build on top of them, allowing interoperability.

use std::{fmt::Debug, marker::PhantomData};

use bevy_ecs::prelude::*;
use derivative::Derivative;

use crate::client::DisconnectReason;

use super::{CloseReason, ServerTransport};

/// System set for server-side networking systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ServerTransportSet {
    /// Handles receiving data from the transport and updating its internal
    /// state using [`ServerTransport::poll`].
    Recv,
    /// Handles flushing buffered messages and sending out buffered data using
    /// [`ServerTransport::flush`].
    Send,
}

/// A [`Condition`]-satisfying system that returns `true` if the server `T`
/// exists *and* is in the [`Open`] state.
///
/// # Example
///
/// ```
/// # use bevy_app::prelude::*;
/// # use bevy_ecs::prelude::*;
/// # use aeronet::server::{ServerTransport, server_open};
/// # fn run<T: ServerTransport + Resource>() {
/// let mut app = App::new();
/// app.add_systems(Update, my_system::<T>.run_if(server_open::<T>));
///
/// fn my_system<T: ServerTransport + Resource>(server: Res<T>) {
///     // ..
/// }
/// # }
/// ```
///
/// [`Condition`]: bevy_ecs::schedule::Condition
/// [`Open`]: crate::server::ServerState::Open
#[must_use]
pub fn server_open<T: ServerTransport + Resource>(server: Option<Res<T>>) -> bool {
    server.is_some_and(|server| server.state().is_open())
}

/// A [`Condition`]-satisfying system that returns `true` if the server `T`
/// exists *and* is in the [`Closed`] state.
///
/// # Example
///
/// ```
/// # use bevy_app::prelude::*;
/// # use bevy_ecs::prelude::*;
/// # use aeronet::server::{ServerTransport, server_closed};
/// # fn run<T: ServerTransport + Resource>() {
/// let mut app = App::new();
/// app.add_systems(Update, my_system::<T>.run_if(server_closed::<T>));
///
/// fn my_system<T: ServerTransport + Resource>(server: Res<T>) {
///     // ..
/// }
/// # }
/// ```
///
/// [`Condition`]: bevy_ecs::schedule::Condition
/// [`Closed`]: crate::server::ServerState::Closed
#[must_use]
pub fn server_closed<T: ServerTransport + Resource>(server: Option<Res<T>>) -> bool {
    server.map_or(true, |server| server.state().is_closed())
}

/// The server has completed setup and is ready to accept client
/// connections, changing state to [`ServerState::Open`].
///
/// See [`ServerEvent::Opened`].
///
/// [`ServerState::Open`]: crate::server::ServerState::Open
/// [`ServerEvent::Opened`]: super::ServerEvent::Opened
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""), Default(bound = ""))]
pub struct ServerOpened<T: ServerTransport> {
    #[derivative(Debug = "ignore")]
    #[doc(hidden)]
    pub _phantom: PhantomData<T>,
}

/// The server can no longer handle client connections, changing state to
/// [`ServerState::Closed`].
///
/// See [`ServerEvent::Closed`].
///
/// [`ServerState::Closed`]: crate::server::ServerState::Closed
/// [`ServerEvent::Closed`]: super::ServerEvent::Closed
#[derive(Derivative, Event)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub struct ServerClosed<T: ServerTransport> {
    /// Why the server closed.
    pub error: CloseReason<T::Error>,
}

/// A remote client has requested to connect to this server.
///
/// The client has been given a key, and the server is trying to establish
/// communication with this client, but messages cannot be transported yet.
///
/// For a given client, the transport is guaranteed to emit this event
/// before [`ServerEvent::Connected`].
///
/// See [`ServerEvent::Connecting`].
///
/// [`ServerEvent::Connecting`]: super::ServerEvent::Connecting
/// [`ServerEvent::Connected`]: super::ServerEvent::Connected
/// [`ServerEvent::Disconnected`]: super::ServerEvent::Disconnected
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct RemoteClientConnecting<T: ServerTransport> {
    /// Key of the client.
    pub client_key: T::ClientKey,
}

/// A remote client has fully established a connection to this server,
/// changing the client's state to [`ClientState::Connected`].
///
/// After this event, you can start sending messages to and receiving
/// messages from the client.
///
/// See [`ServerEvent::Connected`].
///
/// [`ClientState::Connected`]: crate::client::ClientState::Connected
/// [`ServerEvent::Connected`]: super::ServerEvent::Connected
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct RemoteClientConnected<T: ServerTransport> {
    /// Key of the client.
    pub client_key: T::ClientKey,
}

/// A remote client has unrecoverably lost connection from this server.
///
/// This is emitted for *any* reason that the client may be disconnected,
/// including user code calling [`ServerTransport::disconnect`], therefore
/// this may be used as a signal to tear down the client's state.
///
/// See [`ServerEvent::Disconnected`].
///
/// [`ServerEvent::Disconnected`]: super::ServerEvent::Disconnected
#[derive(Derivative, Event)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub struct RemoteClientDisconnected<T: ServerTransport> {
    /// Key of the client.
    pub client_key: T::ClientKey,
    /// Why the client lost connection.
    pub reason: DisconnectReason<T::Error>,
}

/// A client acknowledged that they have fully received a message sent by
/// us.
///
/// See [`ServerEvent::Ack`].
///
/// [`ServerEvent::Ack`]: super::ServerEvent::Ack
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct AckFromClient<T: ServerTransport> {
    /// Key of the client.
    pub client_key: T::ClientKey,
    /// Key of the sent message, obtained by [`ServerTransport::send`].
    pub msg_key: T::MessageKey,
}

/// Our server believes that an unreliable message sent to a client has probably
/// been lost in transit.
///
/// An implementation is allowed to not emit this event if it is not able to.
///
/// See [`ServerEvent::Nack`].
///
/// [`ServerEvent::Nack`]: super::ServerEvent::Nack
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct NackFromClient<T: ServerTransport> {
    /// Key of the client.
    pub client_key: T::ClientKey,
    /// Key of the sent message, obtained by [`ServerTransport::send`].
    pub msg_key: T::MessageKey,
}
