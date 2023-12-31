use std::error::Error;

pub trait Message: Send + Sync + 'static {}

pub trait TryAsBytes {
    type Output<'a>: AsRef<[u8]> + 'a
    where
        Self: 'a;

    type Error: Error + Send + Sync + 'static;

    fn try_as_bytes(&self) -> Result<Self::Output<'_>, Self::Error>;
}

pub trait TryFromBytes {
    type Error: Error + Send + Sync + 'static;

    fn try_from_bytes(buf: &[u8]) -> Result<Self, Self::Error>
    where
        Self: Sized;
}
