use std::{fmt::Debug, marker::PhantomData, time::Instant};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use derivative::Derivative;

use crate::{ClientKey, ServerEvent, ServerTransport, TransportProtocol};

/// Forwards messages and events between the [`App`] and a [`ServerTransport`].
///
/// See [`ServerTransportPlugin`] for a struct version of this plugin.
///
/// With this plugin added, the transport `T` will automatically run
/// [`ServerTransport::update`] on [`PreUpdate`] in the [`ServerTransportSet`],
/// and send out the appropriate events.
///
/// This plugin sends out the events:
/// * [`RemoteConnecting`]
/// * [`RemoteConnected`]
/// * [`RemoteDisconnected`]
/// * [`FromClient`]
///
/// These events can be read by your app to respond to incoming events. To send
/// out messages, or to disconnect a specific client, etc., you will need to
/// inject the transport as a resource into your system.
pub fn server_transport_plugin<P, T>(app: &mut App)
where
    P: TransportProtocol,
    T: ServerTransport<P> + Resource,
{
    app.add_event::<RemoteConnecting<P, T>>()
        .add_event::<RemoteConnected<P, T>>()
        .add_event::<RemoteDisconnected<P, T>>()
        .add_event::<FromClient<P>>()
        .configure_sets(PreUpdate, ServerTransportSet)
        .add_systems(PreUpdate, recv::<P, T>.in_set(ServerTransportSet));
}

/// Forwards messages and events between the [`App`] and a [`ServerTransport`].
///
/// See [`server_transport_plugin`].
#[derive(Derivative)]
#[derivative(Debug(bound = ""), Clone(bound = ""), Default(bound = ""))]
pub struct ServerTransportPlugin<P, T> {
    #[derivative(Debug = "ignore")]
    _phantom_p: PhantomData<P>,
    #[derivative(Debug = "ignore")]
    _phantom_t: PhantomData<T>,
}

impl<P, T> Plugin for ServerTransportPlugin<P, T>
where
    P: TransportProtocol,
    T: ServerTransport<P> + Resource,
{
    fn build(&self, app: &mut App) {
        server_transport_plugin::<P, T>(app);
    }
}

/// Runs the [`server_transport_plugin`] systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub struct ServerTransportSet;

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
#[derivative(
    Debug(bound = "T::ConnectingInfo: Debug"),
    Clone(bound = "T::ConnectingInfo: Clone")
)]
pub struct RemoteConnecting<P, T>
where
    P: TransportProtocol,
    T: ServerTransport<P>,
{
    /// Key of the client.
    pub client: ClientKey,
    /// Info on the connection.
    ///
    /// This is the same data as held by [`ClientState::Connecting`].
    ///
    /// [`ClientState::Connecting`]: crate::ClientState::Connecting
    pub info: T::ConnectingInfo,
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
#[derivative(
    Debug(bound = "T::ConnectedInfo: Debug"),
    Clone(bound = "T::ConnectedInfo: Clone")
)]
pub struct RemoteConnected<P, T>
where
    P: TransportProtocol,
    T: ServerTransport<P>,
{
    /// Key of the client.
    pub client: ClientKey,
    /// Info on the connection.
    ///
    /// This is the same data as held by [`ClientState::Connected`].
    ///
    /// [`ClientState::Connected`]: crate::ClientState::Connected
    pub info: T::ConnectedInfo,
}

/// A remote client has unrecoverably lost connection from this server.
///
/// This event is not raised when the server forces a client to disconnect.
///
/// See [`ServerEvent::Disconnected`].
#[derive(Derivative, Event)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub struct RemoteDisconnected<P, T>
where
    P: TransportProtocol,
    T: ServerTransport<P>,
{
    /// Key of the client.
    pub client: ClientKey,
    /// Why the client lost connection.
    pub reason: T::Error,
}

/// The server received a message from a remote client.
///
/// See [`ServerEvent::Recv`].
#[derive(Derivative, Event)]
#[derivative(Debug(bound = "P::C2S: Debug"), Clone(bound = "P::C2S: Clone"))]
pub struct FromClient<P>
where
    P: TransportProtocol,
{
    /// Key of the client.
    pub client: ClientKey,
    /// The message received.
    pub msg: P::C2S,
    /// When the message was first received.
    ///
    /// Since the transport may use e.g. an async task to receive data, the
    /// time at which the message was polled using
    /// [`ServerTransport::update`] is not necessarily when the app first
    /// became aware of this message.
    pub at: Instant,
}

fn recv<P, T>(
    mut server: ResMut<T>,
    mut connecting: EventWriter<RemoteConnecting<P, T>>,
    mut connected: EventWriter<RemoteConnected<P, T>>,
    mut disconnected: EventWriter<RemoteDisconnected<P, T>>,
    mut recv: EventWriter<FromClient<P>>,
) where
    P: TransportProtocol,
    T: ServerTransport<P> + Resource,
{
    for event in server.update() {
        match event {
            ServerEvent::Connecting { client, info } => {
                connecting.send(RemoteConnecting { client, info })
            }
            ServerEvent::Connected { client, info } => {
                connected.send(RemoteConnected { client, info })
            }
            ServerEvent::Disconnected { client, reason } => {
                disconnected.send(RemoteDisconnected { client, reason })
            }
            ServerEvent::Recv { client, msg, at } => recv.send(FromClient { client, msg, at }),
        }
    }
}
