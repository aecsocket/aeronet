use std::{fmt::Debug, marker::PhantomData};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_time::Time;
use bytes::Bytes;
use derivative::Derivative;

use crate::client::{ClientEvent, ClientTransport};

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
/// * [`NackFromServer`]
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
pub struct ClientTransportPlugin<T> {
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<T>,
}

impl<T: ClientTransport + Resource> Plugin for ClientTransportPlugin<T> {
    fn build(&self, app: &mut App) {
        app.add_event::<LocalClientConnected<T>>()
            .add_event::<LocalClientDisconnected<T>>()
            .add_event::<FromServer<T>>()
            .add_event::<AckFromServer<T>>()
            .add_event::<NackFromServer<T>>()
            .add_event::<ClientFlushError<T>>()
            .configure_sets(PreUpdate, ClientTransportSet::Recv)
            .configure_sets(PostUpdate, ClientTransportSet::Flush)
            .add_systems(PreUpdate, Self::recv.in_set(ClientTransportSet::Recv))
            .add_systems(
                PostUpdate,
                Self::flush
                    .run_if(client_connected::<T>)
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
    client
        .map(|client| client.state().is_connected())
        .unwrap_or(false)
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
    client
        .map(|client| client.state().is_disconnected())
        .unwrap_or(true)
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
pub struct LocalClientConnected<T: ClientTransport> {
    #[derivative(Debug = "ignore")]
    #[doc(hidden)]
    pub _phantom: PhantomData<T>,
}

/// The client has unrecoverably lost connection from its previously
/// connected server.
///
/// This event is not raised when the app invokes a disconnect.
///
/// See [`ClientEvent::Disconnected`].
#[derive(Derivative, Event)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub struct LocalClientDisconnected<T: ClientTransport> {
    /// Why the client lost connection.
    pub error: T::Error,
}

/// The client received a message from the server.
///
/// See [`ClientEvent::Recv`].
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct FromServer<T: ClientTransport> {
    /// The message received.
    pub msg: Bytes,
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
#[derive(Derivative, Event)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct NackFromServer<T: ClientTransport> {
    /// Key of the sent message, obtained by [`ClientTransport::send`].
    pub msg_key: T::MessageKey,
}

/// [`ClientTransport::flush`] produced an error.
#[derive(Derivative, Event)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub struct ClientFlushError<T: ClientTransport> {
    /// Error produced by [`ClientTransport::flush`].
    pub error: T::Error,
}

impl<T: ClientTransport + Resource> ClientTransportPlugin<T> {
    fn recv(
        time: Res<Time>,
        mut client: ResMut<T>,
        mut connected: EventWriter<LocalClientConnected<T>>,
        mut disconnected: EventWriter<LocalClientDisconnected<T>>,
        mut recv: EventWriter<FromServer<T>>,
        mut ack: EventWriter<AckFromServer<T>>,
        mut nack: EventWriter<NackFromServer<T>>,
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
                ClientEvent::Nack { msg_key } => {
                    nack.send(NackFromServer { msg_key });
                }
            }
        }
    }

    fn flush(mut client: ResMut<T>, mut errors: EventWriter<ClientFlushError<T>>) {
        if let Err(error) = client.flush() {
            errors.send(ClientFlushError { error });
        }
    }
}
