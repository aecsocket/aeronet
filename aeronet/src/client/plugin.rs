use std::{fmt::Debug, marker::PhantomData};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_time::Time;
use derivative::Derivative;

use crate::{
    client::{ClientEvent, ClientTransport},
    protocol::TransportProtocol,
};

/// Forwards messages and events between the [`App`] and a [`ClientTransport`].
///
/// With this plugin added, the transport `T` will automatically run:
/// * [`poll`] in [`PreUpdate`] in [`ClientTransportSet::Recv`]
/// * [`flush`] in [`PostUpdate`] in [`ClientTransportSet::Flush`]
///
/// [`poll`]: ClientTransport::poll
/// [`flush`]: ClientTransport::flush
///
/// This plugin sends out the events:
/// * [`LocalClientConnected`]
/// * [`LocalClientDisconnected`]
/// * [`FromServer`]
/// * [`AckFromServer`]
/// * [`ClientConnectionError`]
/// * [`ClientFlushError`]
///
/// This plugin provides the run conditions:
/// * [`client_connected`]
/// * [`client_disconnected`]
///
/// These events can be read by your app to respond to incoming events. To send
/// out messages, or to connect the transport to a remote endpoint, etc., you
/// will need to inject the transport as a resource into your system.
#[derive(Derivative)]
#[derivative(Debug(bound = ""), Clone(bound = ""), Default(bound = ""))]
pub struct ClientTransportPlugin<P, T> {
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<(P, T)>,
}

impl<P, T> Plugin for ClientTransportPlugin<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    fn build(&self, app: &mut App) {
        app.add_event::<LocalClientConnected<P, T>>()
            .add_event::<LocalClientDisconnected<P, T>>()
            .add_event::<FromServer<P, T>>()
            .add_event::<AckFromServer<P, T>>()
            .add_event::<ClientConnectionError<P, T>>()
            .add_event::<ClientFlushError<P, T>>()
            .configure_sets(PreUpdate, ClientTransportSet::Recv)
            .configure_sets(PostUpdate, ClientTransportSet::Flush)
            .add_systems(PreUpdate, Self::recv.in_set(ClientTransportSet::Recv))
            .add_systems(
                PostUpdate,
                Self::flush
                    .run_if(client_connected::<P, T>)
                    .in_set(ClientTransportSet::Flush),
            );
    }
}

/// Runs the [`ClientTransportPlugin`] systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ClientTransportSet {
    /// Handles receiving data from the transport and updating its internal
    /// state.
    Recv,
    /// Handles flushing buffered messages and sending out buffered data.
    Flush,
}

/// A [`Condition`]-satisfying system that returns `true` if the client `T`
/// exists *and* is in the [`Connected`] state.
///
/// # Example
///
/// ```
/// # use bevy_app::prelude::*;
/// # use bevy_ecs::prelude::*;
/// # use aeronet::{protocol::TransportProtocol, client::{ClientTransport, client_connected}};
/// # fn run<P: TransportProtocol, T: ClientTransport<P> + Resource>() {
/// let mut app = App::new();
/// app.add_systems(Update, my_system::<P, T>.run_if(client_connected::<P, T>));
///
/// fn my_system<P, T>(client: Res<T>)
/// where
///     P: TransportProtocol,
///     T: ClientTransport<P> + Resource,
/// {
///     // ..
/// }
/// # }
/// ```
///
/// [`Condition`]: bevy_ecs::schedule::Condition
/// [`Connected`]: crate::client::ClientState::Connected
#[must_use]
pub fn client_connected<P, T>(client: Option<Res<T>>) -> bool
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    if let Some(client) = client {
        client.state().is_connected()
    } else {
        false
    }
}

/// A [`Condition`]-satisfying system that returns `true` if the client `T`
/// does not exist *or* is in the [`Disconnected`] state.
///
/// # Example
///
/// ```
/// # use bevy_app::prelude::*;
/// # use bevy_ecs::prelude::*;
/// # use aeronet::{protocol::TransportProtocol, client::{ClientTransport, client_disconnected}};
/// # fn run<P: TransportProtocol, T: ClientTransport<P> + Resource>() {
/// let mut app = App::new();
/// app.add_systems(Update, my_system::<P, T>.run_if(client_disconnected::<P, T>));
///
/// fn my_system<P, T>(client: Res<T>)
/// where
///     P: TransportProtocol,
///     T: ClientTransport<P> + Resource,
/// {
///     // ..
/// }
/// # }
/// ```
///
/// [`Condition`]: bevy_ecs::schedule::Condition
/// [`Disconnected`]: crate::client::ClientState::Disconnected
#[must_use]
pub fn client_disconnected<P, T>(client: Option<Res<T>>) -> bool
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    if let Some(client) = client {
        client.state().is_disconnected()
    } else {
        true
    }
}

/// The client has fully established a connection to the server.
///
/// This event can be followed by [`ClientEvent::Recv`] or
/// [`ClientEvent::Disconnected`].
///
/// After this event, you can run your game initialization logic such as
/// receiving the initial world state and e.g. showing a spawn screen.
///
/// See [`ClientEvent::Connected`].
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct LocalClientConnected<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    #[derivative(Debug = "ignore")]
    #[doc(hidden)]
    pub _phantom: PhantomData<(P, T)>,
}

/// The client has unrecoverably lost connection from its previously
/// connected server.
///
/// This event is not raised when the app invokes a disconnect.
///
/// See [`ClientEvent::Disconnected`].
#[derive(Derivative, Event)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub struct LocalClientDisconnected<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    /// Why the client lost connection.
    pub error: T::Error,
}

/// The client received a message from the server.
///
/// See [`ClientEvent::Recv`].
#[derive(Derivative, Event)]
#[derivative(Debug(bound = "P::S2C: Debug"), Clone(bound = "P::S2C: Clone"))]
pub struct FromServer<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    /// The message received.
    pub msg: P::S2C,
    #[derivative(Debug = "ignore")]
    #[doc(hidden)]
    pub _phantom: PhantomData<T>,
}

/// The peer acknowledged that they have fully received a message sent by
/// us.
///
/// See [`ClientEvent::Ack`].
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct AckFromServer<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    /// Key of the sent message, obtained by [`ClientTransport::send`].
    pub msg_key: T::MessageKey,
}

/// The client has experienced a non-fatal network error.
///
/// The connection is still active until [`ClientEvent::Disconnected`] is
/// emitted.
///
/// See [`ClientEvent::ConnectionError`].
#[derive(Derivative, Event)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub struct ClientConnectionError<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    /// Error which occurred.
    pub error: T::Error,
}

/// [`ClientTransport::flush`] produced an error.
#[derive(Derivative, Event)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub struct ClientFlushError<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    /// Error produced by [`ClientTransport::flush`].
    pub error: T::Error,
}

impl<P, T> ClientTransportPlugin<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    fn recv(
        time: Res<Time>,
        mut client: ResMut<T>,
        mut connected: EventWriter<LocalClientConnected<P, T>>,
        mut disconnected: EventWriter<LocalClientDisconnected<P, T>>,
        mut recv: EventWriter<FromServer<P, T>>,
        mut ack: EventWriter<AckFromServer<P, T>>,
        mut errors: EventWriter<ClientConnectionError<P, T>>,
    ) {
        for event in client.poll(time.delta()) {
            match event {
                ClientEvent::Connected => {
                    connected.send(LocalClientConnected {
                        _phantom: PhantomData,
                    });
                }
                ClientEvent::Disconnected { error } => {
                    disconnected.send(LocalClientDisconnected { error });
                }
                ClientEvent::Recv { msg } => {
                    recv.send(FromServer {
                        msg,
                        _phantom: PhantomData,
                    });
                }
                ClientEvent::Ack { msg_key } => {
                    ack.send(AckFromServer { msg_key });
                }
                ClientEvent::ConnectionError { error } => {
                    errors.send(ClientConnectionError { error });
                }
            }
        }
    }

    fn flush(mut client: ResMut<T>, mut errors: EventWriter<ClientFlushError<P, T>>) {
        if let Err(error) = client.flush() {
            errors.send(ClientFlushError { error });
        }
    }
}
