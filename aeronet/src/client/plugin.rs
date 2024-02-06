use std::{fmt::Debug, marker::PhantomData};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use derivative::Derivative;

use crate::{ClientEvent, ClientTransport, TransportProtocol};

/// Forwards messages and events between the [`App`] and a [`ClientTransport`].
///
/// See [`ClientTransportPlugin`] for a struct version of this plugin.
///
/// With this plugin added, the transport `T` will automatically run
/// [`ClientTransport::update`] on [`PreUpdate`] in the [`ClientTransportSet`],
/// and send out the appropriate events.
///
/// This plugin sends out the events:
/// * [`LocalConnected`]
/// * [`LocalDisconnected`]
/// * [`FromServer`]
///
/// These events can be read by your app to respond to incoming events. To send
/// out messages, or to connect the transport to a remote endpoint, etc., you
/// will need to inject the transport as a resource into your system.
pub fn client_transport_plugin<P, T>(app: &mut App)
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    app.add_event::<LocalClientConnected<P, T>>()
        .add_event::<LocalClientDisconnected<P, T>>()
        .add_event::<FromServer<P, T>>()
        .configure_sets(PreUpdate, ClientTransportSet)
        .add_systems(PreUpdate, recv::<P, T>.in_set(ClientTransportSet));
}

/// Forwards messages and events between the [`App`] and a [`ClientTransport`].
///
/// See [`client_transport_plugin`].
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

/// Runs the [`client_transport_plugin`] systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub struct ClientTransportSet;

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
    #[doc(hidden)]
    pub _phantom: PhantomData<T>,
}

fn recv<P, T>(
    mut client: ResMut<T>,
    mut connected: EventWriter<LocalClientConnected<P, T>>,
    mut disconnected: EventWriter<LocalClientDisconnected<P, T>>,
    mut recv: EventWriter<FromServer<P, T>>,
) where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    for event in client.update() {
        match event {
            ClientEvent::Connected => connected.send(LocalClientConnected {
                _phantom: PhantomData,
            }),
            ClientEvent::Disconnected { reason } => {
                disconnected.send(LocalClientDisconnected { reason })
            }
            ClientEvent::Recv { msg } => recv.send(FromServer {
                msg,
                _phantom: PhantomData,
            }),
        }
    }
}
