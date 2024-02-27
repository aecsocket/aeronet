use std::{fmt::Debug, marker::PhantomData};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use derivative::Derivative;

use crate::{
    client::{ClientEvent, ClientTransport},
    TransportProtocol,
};

/// Forwards messages and events between the [`App`] and a [`ClientTransport`].
///
/// See [`ClientTransportPlugin`].
pub fn client_transport_plugin<P, T>(app: &mut App)
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
    T::Error: Send + Sync,
{
    app.add_event::<LocalClientConnected<P, T>>()
        .add_event::<LocalClientDisconnected<P, T>>()
        .add_event::<FromServer<P, T>>()
        .add_event::<AckFromServer<P, T>>()
        .configure_sets(PreUpdate, ClientTransportSet::Poll)
        .add_systems(PreUpdate, recv::<P, T>.in_set(ClientTransportSet::Poll));
}

/// Forwards messages and events between the [`App`] and a [`ClientTransport`].
///
/// See [`client_transport_plugin`] for a function version of this plugin.
///
/// With this plugin added, the transport `T` will automatically run
/// [`ClientTransport::poll`] on [`PreUpdate`] in the set
/// [`ClientTransportSet::Poll`], and send out the appropriate events.
///
/// This plugin sends out the events:
/// * [`LocalClientConnected`]
/// * [`LocalClientDisconnected`]
/// * [`FromServer`]
/// * [`AckFromServer`]
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
        client_transport_plugin::<P, T>(app);
    }
}

/// Runs the [`ClientTransportPlugin`] systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ClientTransportSet {
    /// Handles receiving data from the transport and updating its internal
    /// state.
    Poll,
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
    _phantom: PhantomData<(P, T)>,
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
    pub reason: T::Error,
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
    _phantom: PhantomData<T>,
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

fn recv<P, T>(
    mut client: ResMut<T>,
    mut connected: EventWriter<LocalClientConnected<P, T>>,
    mut disconnected: EventWriter<LocalClientDisconnected<P, T>>,
    mut recv: EventWriter<FromServer<P, T>>,
    mut ack: EventWriter<AckFromServer<P, T>>,
) where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    for event in client.poll() {
        match event {
            ClientEvent::Connected => {
                connected.send(LocalClientConnected {
                    _phantom: PhantomData,
                });
            }
            ClientEvent::Disconnected { reason } => {
                disconnected.send(LocalClientDisconnected { reason });
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
        }
    }
}
