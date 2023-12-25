use std::{convert::Infallible, fmt::Display, string::FromUtf8Error};

use aeronet::{
    ChannelKey, ChannelProtocol, Message, OnChannel, TransportProtocol, TryAsBytes, TryFromBytes,
};

/// The channel type used for echo messages sent by our app.
///
/// In this case, there's only a single `Unreliable` channel, but this can be
/// an enum with multiple fieldless variants.
///
/// You only need a channel if you're using a transport which requires a channel
/// to be defined (see [`ChannelProtocol`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ChannelKey)]
#[channel_kind(Unreliable)]
pub struct EchoChannel;

/// The echo message type sent between clients and servers.
///
/// Our app only has a single message type for client-to-server (C2S) and
/// server-to-client (S2C) communication, but you can (and probably should) have
/// two separate types.
///
/// This type derives [`Message`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Message, OnChannel)]
#[channel_type(EchoChannel)]
#[on_channel(EchoChannel)]
pub struct EchoMessage(pub String);

/// Defines the configuration for transports that we use in our echo app.
///
/// This uses the [`EchoMessage`] and [`EchoChannel`] structs we made earlier.
pub struct EchoProtocol;

impl TransportProtocol for EchoProtocol {
    type C2S = EchoMessage;
    type S2C = EchoMessage;
}

impl ChannelProtocol for EchoProtocol {
    type Channel = EchoChannel;
}

// Helper stuff

impl Display for EchoMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<T> From<T> for EchoMessage
where
    T: Into<String>,
{
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

impl TryAsBytes for EchoMessage {
    type Output<'a> = &'a [u8];

    type Error = Infallible;

    fn try_as_bytes(&self) -> Result<Self::Output<'_>, Self::Error> {
        Ok(self.0.as_bytes())
    }
}

impl TryFromBytes for EchoMessage {
    type Error = FromUtf8Error;

    fn try_from_bytes(buf: &[u8]) -> Result<Self, Self::Error> {
        String::from_utf8(buf.to_vec()).map(EchoMessage)
    }
}
