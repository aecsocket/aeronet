use std::{convert::Infallible, string::FromUtf8Error};

use aeronet::{
    ChannelKey, ChannelProtocol, OnChannel, TransportProtocol, TryAsBytes, TryFromBytes,
};

/// The channel type used for messages sent by our app.
///
/// In this case, there's only a single `Unreliable` channel, but this can be
/// an enum with multiple fieldless variants.
///
/// You only need a channel if you're using a transport which requires a channel
/// to be defined (see [`ChannelProtocol`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ChannelKey)]
#[channel_kind(Unreliable)]
pub struct AppChannel;

/// The message type sent between clients and servers.
///
/// Our app only has a single message type for client-to-server (C2S) and
/// server-to-client (S2C) communication, but you can (and probably should) have
/// two separate types.
#[derive(Debug, Clone, PartialEq, Eq, Hash, OnChannel)]
#[channel_type(AppChannel)]
#[on_channel(AppChannel)]
pub struct AppMessage(pub String);

/// Defines the configuration for transports that we use in our app.
///
/// This uses the [`AppMessage`] and [`AppChannel`] structs we made earlier.
pub struct AppProtocol;

impl TransportProtocol for AppProtocol {
    type C2S = AppMessage;
    type S2C = AppMessage;
}

impl ChannelProtocol for AppProtocol {
    type Channel = AppChannel;
}

// Helper stuff

impl<T> From<T> for AppMessage
where
    T: Into<String>,
{
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

impl TryAsBytes for AppMessage {
    type Output<'a> = &'a [u8];

    type Error = Infallible;

    fn try_as_bytes(&self) -> Result<Self::Output<'_>, Self::Error> {
        Ok(self.0.as_bytes())
    }
}

impl TryFromBytes for AppMessage {
    type Error = FromUtf8Error;

    fn try_from_bytes(buf: &[u8]) -> Result<Self, Self::Error> {
        String::from_utf8(buf.to_vec()).map(AppMessage)
    }
}
