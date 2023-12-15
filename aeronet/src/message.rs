use std::error::Error;

/// Data that can be sent to and received by a transport.
///
/// This is a marker trait that ensures that data sent between transports is:
/// * [`Send`]
/// * [`Sync`]
/// * `'static`
///
/// This trait is automatically implemented for all types matching these
/// criteria.
///
/// The user defines which types of messages their transport uses, and this
/// trait acts as a minimum bound for all message types. However, for different
/// transport implementations, there may be additional bounds placed on message
/// types, such as for transports using a network, in which data is sent as a
/// byte sequence:
/// * [`TryAsBytes`] if the message should be able to be converted into a byte
///   sequence
/// * [`TryFromBytes`] if the message should be able to be constructed from a
///   byte sequence
pub trait Message: Send + Sync + 'static {}

impl<T> Message for T where T: Send + Sync + 'static {}

/// Type that can potentially be read as a sequence of bytes.
///
/// The transport implementation may wish to handle messages as a byte sequence,
/// for example if it communicates over a network. This trait can be used as a
/// bound to ensure that messages can be converted into a byte form.
///
/// See [`TryFromBytes`] for the receiving counterpart.
#[cfg_attr(
    feature = "bincode",
    doc = r##"

# [`bincode`] + [`serde`] support

With the `bincode` feature enabled, this trait will automatically be implemented for types which
implement [`serde::Serialize`].
"##
)]
pub trait TryAsBytes {
    /// Output type of [`TryAsBytes::try_as_bytes`], which can be
    /// converted into a slice of bytes.
    type Output<'a>: AsRef<[u8]> + Send
    where
        Self: 'a;

    /// Error type for [`TryAsBytes::try_as_bytes`].
    type Error: Error + Send + Sync + 'static;

    /// Performs the conversion.
    ///
    /// # Errors
    ///
    /// Errors if the conversion could not be performed.
    fn try_as_bytes(&self) -> Result<Self::Output<'_>, Self::Error>;
}

/// Data that can potentially be converted from a sequence of bytes into this
/// type.
///
/// The transport implementation may wish to handle messages as a byte sequence,
/// for example if it communicates over a network. This trait can be used as a
/// bound to ensure that messages can be created from a byte form.
///
/// Note that this trait only requires a *reference* to the bytes rather than
/// ownership of them. This should be fine in most use-cases, but is still
/// something to be aware of.
///
/// See [`TryAsBytes`] for the sending counterpart.
#[cfg_attr(
    feature = "bincode",
    doc = r##"

# [`bincode`] + [`serde`] support

With the `bincode` feature enabled, this trait will automatically be implemented for types which
implement [`serde::de::DeserializeOwned`].
"##
)]
pub trait TryFromBytes: Sized {
    /// Error type for [`TryFromBytes::try_from_bytes`].
    type Error: Error + Send + Sync + 'static;

    /// Performs the conversion.
    ///
    /// # Errors
    ///
    /// Errors if the conversion could not be performed.
    fn try_from_bytes(buf: &[u8]) -> Result<Self, Self::Error>;
}

#[cfg(feature = "bincode")]
impl<T> TryAsBytes for T
where
    T: serde::Serialize,
{
    type Output<'a> = Vec<u8> where Self: 'a;

    type Error = bincode::Error;

    fn try_as_bytes(&self) -> Result<Self::Output<'_>, Self::Error> {
        bincode::serialize(self)
    }
}

#[cfg(feature = "bincode")]
impl<T> TryFromBytes for T
where
    T: serde::de::DeserializeOwned,
{
    type Error = bincode::Error;

    fn try_from_bytes(buf: &[u8]) -> Result<Self, Self::Error> {
        bincode::deserialize(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    #[cfg_attr(feature = "bincode", derive(serde::Serialize, serde::Deserialize))]
    struct Value {
        x: u32,
        y: i32,
    }

    #[test]
    fn type_is_message() {
        assert_message::<Value>();
    }

    fn assert_message<T: Message>() {}

    #[test]
    #[cfg(feature = "bincode")]
    fn bincode_serde() {
        let value = Value { x: 4, y: -2 };
        let bytes = value.try_as_bytes().unwrap();
        let value = Value::try_from_bytes(&bytes).unwrap();
        assert_eq!(Value { x: 4, y: -2 }, value);
    }
}
