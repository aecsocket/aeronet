use std::marker::PhantomData;

use bevy::prelude::*;
use derivative::Derivative;

use crate::{ClientEvent, TransportClient, TransportProtocol};

/// Provides systems to send commands to, and receive events from, a
/// [`TransportClient`].
///
/// To use a struct version of this plugin, see [`TransportClientPlugin`].
///
/// With this plugin added, the transport `T` will receive data and update its
/// state on [`PreUpdate`], and send out messages triggered by the app on
/// [`PostUpdate`]. This is controlled by the [`TransportClientSet`].
///
/// This plugin emits the events:
/// * [`LocalClientConnecting`]
/// * [`LocalClientConnected`]
/// * [`FromServer`]
/// * [`LocalClientDisconnected`]
///
/// ...and consumes the events:
/// * [`ToServer`]
/// * [`DisconnectLocalClient`]
///
/// To connect the client to a server, you will have to know the concrete type
/// of the client transport, and call the function on it manually.
///
/// Note that errors during operation will be silently ignored, e.g. if you
/// attempt to send a message while the client is not connected.
pub fn transport_client_plugin<P, T>(app: &mut App)
where
    P: TransportProtocol,
    P::C2S: Clone,
    T: TransportClient<P> + Resource,
{
    app.configure_sets(PreUpdate, TransportClientSet::Recv)
        .configure_sets(PostUpdate, TransportClientSet::Send)
        .add_event::<LocalClientConnecting>()
        .add_event::<LocalClientConnected>()
        .add_event::<FromServer<P>>()
        .add_event::<LocalClientDisconnected<P, T>>()
        .add_event::<ToServer<P>>()
        .add_event::<DisconnectLocalClient>()
        .add_systems(PreUpdate, recv::<P, T>.in_set(TransportClientSet::Recv))
        .add_systems(
            PostUpdate,
            (
                send::<P, T>,
                disconnect::<P, T>.run_if(on_event::<DisconnectLocalClient>()),
            )
                .chain()
                .in_set(TransportClientSet::Send),
        );
}

/// Provides systems to send commands to, and receive events from, a
/// [`TransportClient`].
///
/// See [`transport_client_plugin`].
#[derive(Derivative)]
#[derivative(Debug, Default)]
pub struct TransportClientPlugin<P, T>
where
    P: TransportProtocol,
    P::C2S: Clone,
    T: TransportClient<P> + Resource,
{
    #[derivative(Debug = "ignore")]
    _phantom_p: PhantomData<P>,
    #[derivative(Debug = "ignore")]
    _phantom_t: PhantomData<T>,
}

impl<P, T> Plugin for TransportClientPlugin<P, T>
where
    P: TransportProtocol,
    P::C2S: Clone,
    T: TransportClient<P> + Resource,
{
    fn build(&self, app: &mut App) {
        transport_client_plugin::<P, T>(app);
    }
}

/// Group of systems for sending and receiving data to/from a
/// [`TransportClient`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum TransportClientSet {
    /// Receiving data from the connection and updating the client's internal
    /// state.
    Recv,
    /// Sending out messages created by the app.
    Send,
}

/// This client has started connecting to a server.
/// 
/// This may be followed by a [`ClientEvent::Connected`] or a
/// [`ClientEvent::Disconnected`].
///
/// See [`ClientEvent::Connecting`].
#[derive(Debug, Clone, Event)]
pub struct LocalClientConnecting;

/// This client has fully connected to a server.
///
/// Use this event to do setup logic, e.g. start loading the level.
///
/// See [`ClientEvent::Connected`].
#[derive(Debug, Clone, Event)]
pub struct LocalClientConnected;

/// The server sent a message to this client.
///
/// See [`ClientEvent::Recv`].
#[derive(Debug, Clone, Event)]
pub struct FromServer<P>
where
    P: TransportProtocol,
{
    /// The message received.
    pub msg: P::S2C,
}

/// This client has lost connection from its previously connected server,
/// which cannot be recovered from.
///
/// Use this event to do teardown logic, e.g. changing state to the main
/// menu.
///
/// See [`ClientEvent::Disconnected`].
#[derive(Debug, Clone, Event)]
pub struct LocalClientDisconnected<P, T>
where
    P: TransportProtocol,
    T: TransportClient<P>,
{
    /// The reason why the client lost connection.
    pub cause: T::Error,
}

/// Sends a message along the client to the server.
///
/// See [`TransportClient::send`].
#[derive(Debug, Clone, Event)]
pub struct ToServer<P>
where
    P: TransportProtocol,
{
    /// The message to send.
    pub msg: P::C2S,
}

/// Forcefully disconnects the client from its currently connected server.
///
/// See [`TransportClient::disconnect`].
#[derive(Debug, Clone, Event)]
pub struct DisconnectLocalClient;

// systems

fn recv<P, T>(
    mut client: ResMut<T>,
    mut connecting: EventWriter<LocalClientConnecting>,
    mut connected: EventWriter<LocalClientConnected>,
    mut recv: EventWriter<FromServer<P>>,
    mut disconnected: EventWriter<LocalClientDisconnected<P, T>>,
) where
    P: TransportProtocol,
    P::C2S: Clone,
    T: TransportClient<P> + Resource,
{
    for event in client.recv() {
        match event.into() {
            None => {}
            Some(ClientEvent::Connecting) => connecting.send(LocalClientConnecting),
            Some(ClientEvent::Connected) => connected.send(LocalClientConnected),
            Some(ClientEvent::Recv { msg }) => recv.send(FromServer { msg }),
            Some(ClientEvent::Disconnected { cause }) => {
                disconnected.send(LocalClientDisconnected { cause });
            }
        }
    }
}

fn send<P, T>(mut client: ResMut<T>, mut send: EventReader<ToServer<P>>)
where
    P: TransportProtocol,
    P::C2S: Clone,
    T: TransportClient<P> + Resource,
{
    for ToServer { msg } in send.read() {
        let _ = client.send(msg.clone());
    }
}

fn disconnect<P, T>(mut client: ResMut<T>)
where
    P: TransportProtocol,
    P::C2S: Clone,
    T: TransportClient<P> + Resource,
{
    let _ = client.disconnect();
}
