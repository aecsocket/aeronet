use std::{fmt::Debug, marker::PhantomData};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_time::Time;
use derivative::Derivative;
use octs::Bytes;

use super::{ServerEvent, ServerTransport};

/// Forwards messages and events between the [`App`] and a [`ServerTransport`].
///
/// With this plugin added, the transport `T` will automatically run:
/// * [`poll`] in [`PreUpdate`] in [`ServerTransportSet::Recv`]
/// * [`flush`] in [`PostUpdate`] in [`ServerTransportSet::Flush`]
///
/// [`poll`]: ServerTransport::poll
/// [`flush`]: ServerTransport::flush
///
/// This plugin sends out the events:
/// * [`ServerOpened`]
/// * [`ServerClosed`]
/// * [`RemoteClientConnecting`]
/// * [`RemoteClientConnected`]
/// * [`RemoteClientDisconnected`]
/// * [`FromClient`]
/// * [`AckFromClient`]
/// * [`NackFromClient`]
/// * [`ServerFlushError`]
///
/// This plugin provides the run conditions:
/// * [`server_open`]
/// * [`server_closed`]
///
/// These events can be read by your app to respond to incoming events. To send
/// out messages, or to disconnect a specific client, etc., you will need to
/// inject the transport as a resource into your system.
#[derive(Derivative)]
#[derivative(Debug(bound = ""), Clone(bound = ""), Default(bound = ""))]
pub struct ServerTransportPlugin<T: ServerTransport> {
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<T>,
}

impl<T: ServerTransport + Resource> Plugin for ServerTransportPlugin<T> {
    fn build(&self, app: &mut App) {
        app.add_event::<ServerOpened<T>>()
            .add_event::<ServerClosed<T>>()
            .add_event::<RemoteClientConnecting<T>>()
            .add_event::<RemoteClientConnected<T>>()
            .add_event::<RemoteClientDisconnected<T>>()
            .add_event::<FromClient<T>>()
            .add_event::<AckFromClient<T>>()
            .add_event::<ServerFlushError<T>>()
            .configure_sets(PreUpdate, ServerTransportSet::Recv)
            .configure_sets(PostUpdate, ServerTransportSet::Flush)
            .add_systems(PreUpdate, Self::recv.in_set(ServerTransportSet::Recv))
            .add_systems(
                PostUpdate,
                Self::flush
                    .run_if(server_open::<T>)
                    .in_set(ServerTransportSet::Flush),
            );
    }
}

/// Runs the [`ServerTransportPlugin`] systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ServerTransportSet {
    /// Handles receiving data from the transport and updating its internal
    /// state.
    Recv,
    /// Handles flushing buffered messages and sending out buffered data.
    Flush,
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
    server
        .map(|server| server.state().is_open())
        .unwrap_or(false)
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
    server
        .map(|server| server.state().is_closed())
        .unwrap_or(true)
}

/// The server has completed setup and is ready to accept client
/// connections, changing state to [`ServerState::Open`].
///
/// See [`ServerEvent::Opened`]
///
/// [`ServerState::Open`]: crate::server::ServerState::Open
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
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
#[derive(Derivative, Event)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub struct ServerClosed<T: ServerTransport> {
    /// Why the server closed.
    pub error: T::Error,
}

/// A remote client has requested to connect to this server.
///
/// The client has been given a key, and the server is trying to establish
/// communication with this client, but messages cannot be transported yet.
///
/// This event can be followed by [`ServerEvent::Connected`] or
/// [`ServerEvent::Disconnected`].
///
/// See [`ServerEvent::Connecting`].
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct RemoteClientConnecting<T: ServerTransport> {
    /// Key of the client.
    pub client_key: T::ClientKey,
}

/// A remote client has fully established a connection to this server.
///
/// This event can be followed by [`ServerEvent::Recv`] or
/// [`ServerEvent::Disconnected`].
///
/// After this event, you can run your player initialization logic such as
/// spawning the player's model in the world.
///
/// See [`ServerEvent::Connected`].
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct RemoteClientConnected<T: ServerTransport> {
    /// Key of the client.
    pub client_key: T::ClientKey,
}

/// A remote client has unrecoverably lost connection from this server.
///
/// This event is not raised when the server forces a client to disconnect.
///
/// See [`ServerEvent::Disconnected`].
#[derive(Derivative, Event)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub struct RemoteClientDisconnected<T: ServerTransport> {
    /// Key of the client.
    pub client_key: T::ClientKey,
    /// Why the client lost connection.
    pub error: T::Error,
}

/// The server received a message from a remote client.
///
/// See [`ServerEvent::Recv`].
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct FromClient<T: ServerTransport> {
    /// Key of the client.
    pub client_key: T::ClientKey,
    /// The message received.
    pub msg: Bytes,
}

/// A client acknowledged that they have fully received a message sent by
/// us.
///
/// See [`ServerEvent::Ack`].
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
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct NackFromClient<T: ServerTransport> {
    /// Key of the client.
    pub client_key: T::ClientKey,
    /// Key of the sent message, obtained by [`ServerTransport::send`].
    pub msg_key: T::MessageKey,
}

/// [`ServerTransport::flush`] produced an error.
#[derive(Derivative, Event)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub struct ServerFlushError<T: ServerTransport> {
    /// Error produced by [`ServerTransport::flush`].
    pub error: T::Error,
}

impl<T: ServerTransport + Resource> ServerTransportPlugin<T> {
    fn recv(
        time: Res<Time>,
        mut server: ResMut<T>,
        mut opened: EventWriter<ServerOpened<T>>,
        mut closed: EventWriter<ServerClosed<T>>,
        mut connecting: EventWriter<RemoteClientConnecting<T>>,
        mut connected: EventWriter<RemoteClientConnected<T>>,
        mut disconnected: EventWriter<RemoteClientDisconnected<T>>,
        mut recv: EventWriter<FromClient<T>>,
        mut ack: EventWriter<AckFromClient<T>>,
        mut nack: EventWriter<NackFromClient<T>>,
    ) {
        for event in server.poll(time.delta()) {
            match event {
                ServerEvent::Opened => {
                    opened.send(ServerOpened {
                        _phantom: PhantomData,
                    });
                }
                ServerEvent::Closed { error } => {
                    closed.send(ServerClosed { error });
                }
                ServerEvent::Connecting { client_key } => {
                    connecting.send(RemoteClientConnecting { client_key });
                }
                ServerEvent::Connected { client_key } => {
                    connected.send(RemoteClientConnected { client_key });
                }
                ServerEvent::Disconnected { client_key, error } => {
                    disconnected.send(RemoteClientDisconnected { client_key, error });
                }
                ServerEvent::Recv { client_key, msg } => {
                    recv.send(FromClient { client_key, msg });
                }
                ServerEvent::Ack {
                    client_key,
                    msg_key,
                } => {
                    ack.send(AckFromClient {
                        client_key,
                        msg_key,
                    });
                }
                ServerEvent::Nack {
                    client_key,
                    msg_key,
                } => {
                    nack.send(NackFromClient {
                        client_key,
                        msg_key,
                    });
                }
            }
        }
    }

    fn flush(mut server: ResMut<T>, mut errors: EventWriter<ServerFlushError<T>>) {
        if let Err(error) = server.flush() {
            errors.send(ServerFlushError { error });
        }
    }
}
