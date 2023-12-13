use std::marker::PhantomData;

use bevy::prelude::*;
use derivative::Derivative;

use crate::{TransportClient, Message, ClientEvent};

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
pub fn transport_client_plugin<C2S, S2C, T>(app: &mut App)
where
    C2S: Message + Clone,
    S2C: Message,
    T: TransportClient<C2S, S2C> + Resource,
{
    app.configure_sets(PreUpdate, TransportClientSet::Recv)
        .configure_sets(PostUpdate, TransportClientSet::Send)
        .add_event::<LocalClientConnected>()
        .add_event::<FromServer<S2C>>()
        .add_event::<LocalClientDisconnected<T::Error>>()
        .add_event::<ToServer<C2S>>()
        .add_event::<DisconnectLocalClient>()
        .add_systems(
            PreUpdate,
            recv::<C2S, S2C, T>.in_set(TransportClientSet::Recv),
        )
        .add_systems(
            PostUpdate,
            (
                send::<C2S, S2C, T>,
                disconnect::<C2S, S2C, T>.run_if(on_event::<DisconnectLocalClient>()),
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
pub struct TransportClientPlugin<C2S, S2C, T>
where
    C2S: Message + Clone,
    S2C: Message,
    T: TransportClient<C2S, S2C> + Resource,
{
    #[derivative(Debug = "ignore")]
    _phantom_c2s: PhantomData<C2S>,
    #[derivative(Debug = "ignore")]
    _phantom_s2c: PhantomData<S2C>,
    #[derivative(Debug = "ignore")]
    _phantom_t: PhantomData<T>,
}

impl<C2S, S2C, T> Plugin for TransportClientPlugin<C2S, S2C, T>
where
    C2S: Message + Clone,
    S2C: Message,
    T: TransportClient<C2S, S2C> + Resource,
{
    fn build(&self, app: &mut App) {
        transport_client_plugin::<C2S, S2C, T>(app);
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
pub struct FromServer<S2C> {
    /// The message received.
    pub msg: S2C,
}

/// This client has lost connection from its previously connected server,
/// which cannot be recovered from.
///
/// Use this event to do teardown logic, e.g. changing state to the main
/// menu.
/// 
/// See [`ClientEvent::Disconnected`].
#[derive(Debug, Clone, Event)]
pub struct LocalClientDisconnected<E> {
    /// The reason why the client lost connection.
    pub cause: E,
}

/// Sends a message along the client to the server.
/// 
/// See [`TransportClient::send`].
#[derive(Debug, Clone, Event)]
pub struct ToServer<C2S> {
    /// The message to send.
    pub msg: C2S,
}

/// Forcefully disconnects the client from its currently connected server.
/// 
/// See [`TransportClient::disconnect`].s
#[derive(Debug, Clone, Event)]
pub struct DisconnectLocalClient;

// systems

fn recv<C2S, S2C, T>(
    mut client: ResMut<T>,
    mut connected: EventWriter<LocalClientConnected>,
    mut recv: EventWriter<FromServer<S2C>>,
    mut disconnected: EventWriter<LocalClientDisconnected<T::Error>>,
)
where
    C2S: Message + Clone,
    S2C: Message,
    T: TransportClient<C2S, S2C> + Resource,
{
    for event in client.recv() {
        match event.into() {
            None => {},
            Some(ClientEvent::Connected) => connected.send(LocalClientConnected),
            Some(ClientEvent::Recv { msg }) => recv.send(FromServer { msg }),
            Some(ClientEvent::Disconnected { cause }) => {
                disconnected.send(LocalClientDisconnected { cause });
            }
        }
    }
}

fn send<C2S, S2C, T>(
    mut client: ResMut<T>,
    mut send: EventReader<ToServer<C2S>>,
)
where
    C2S: Message + Clone,
    S2C: Message,
    T: TransportClient<C2S, S2C> + Resource,
{
    for ToServer { msg } in send.read() {
        let _ = client.send(msg.clone());
    }
}

fn disconnect<C2S, S2C, T>(
    mut client: ResMut<T>,
)
where
    C2S: Message + Clone,
    S2C: Message,
    T: TransportClient<C2S, S2C> + Resource,
{
    let _ = client.disconnect();
}
