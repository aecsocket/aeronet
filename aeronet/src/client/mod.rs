#[cfg(feature = "bevy")]
pub mod plugin;

use crate::{Message, SessionError};

/// A client-to-server layer responsible for sending user messages to the other
/// side.
///
/// The client transport attempts to connect to a server when created, handles
/// sending and receiving messages, as well as forwarding disconnections and
/// errors to the app.
///
/// Different transport implementations will use different methods to
/// transport the data across, such as through memory or over a network. This
/// means that a transport does not necessarily work over the internet! If you
/// want to get details such as RTT or remote address, see [`Rtt`] and
/// [`RemoteAddr`].
///
/// The type parameters allow configuring which types of messages are sent and
/// received by this transport (see [`Message`]).
///
/// [`Rtt`]: crate::Rtt
/// [`RemoteAddr`]: crate::RemoteAddr
pub trait ClientTransport<C2S: Message, S2C: Message> {
    /// Iterator type over this transport's events.
    type EventIter<'a>: Iterator<Item = ClientEvent<S2C>> + 'a
    where
        Self: 'a;

    /// The info that [`ClientTransport::info`] provides.
    type Info;

    /// Instructs the transport to receive incoming events and update its
    /// internal state.
    ///
    /// This should be called before [`ClientTransport::take_events`].
    fn recv(&mut self);

    /// Takes ownership of all queued events in this transport.
    fn take_events(&mut self) -> Self::EventIter<'_>;

    /// Sends a message to the connected server.
    fn send(&mut self, msg: impl Into<C2S>);

    /// Gets transport info on the current connection.
    ///
    /// If this transport is not connected to a server, [`None`] is returned.
    fn info(&self) -> Option<Self::Info>;

    /// Gets if this transport has a connection to a server.
    fn connected(&self) -> bool;
}

/// An event received from a [`ClientTransport`].
///
/// Under Bevy this also implements `Event`, however this type cannot be used in
/// an event reader or writer using the inbuilt plugins. `Event` is implemented
/// to allow user code to use this type as an event if they wish to manually
/// implement transport handling.
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum ClientEvent<S2C> {
    /// The client successfully connected to the server that was requested when
    /// creating the transport.
    ///
    /// This should be used as a signal to transition into the next app state,
    /// such as entering the level loading menu in a game.
    Connected,
    /// The connected server sent data to the client.
    Recv(S2C),
    /// The connection to the server was closed for any reason.
    ///
    /// This is called for both transport errors (such as losing connection) and
    /// for the transport being forcefully disconnected by the server.
    ///
    /// This should be used as a signal to transition into the next app state,
    /// such as entering the main menu after exiting a server.
    Disconnected(SessionError),
}