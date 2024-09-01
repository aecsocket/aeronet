use std::borrow::Borrow;

use bytes::Bytes;
use either::Either;
use web_time::Duration;

use crate::lane::LaneIndex;

use super::{ServerEvent, ServerState, ServerTransport};

impl<L: ServerTransport, R: ServerTransport> ServerTransport for Either<L, R> {
    type Opening<'this> = Either<L::Opening<'this>, R::Opening<'this>> where Self: 'this;

    type Open<'this> = Either<L::Open<'this>, R::Open<'this>> where Self: 'this;

    type Connecting<'this> = Either<L::Connecting<'this>, R::Connecting<'this>> where Self: 'this;

    type Connected<'this> = Either<L::Connected<'this>, R::Connected<'this>> where Self: 'this;

    type ClientKey = Either<L::ClientKey, R::ClientKey>;

    type MessageKey = Either<L::MessageKey, R::MessageKey>;

    type PollError = Either<L::PollError, R::PollError>;

    type SendError = Either<L::SendError, R::SendError>;

    fn state(&self) -> ServerState<Self::Opening<'_>, Self::Open<'_>> {
        match self {
            Self::Left(l) => l.state().map(Either::Left, Either::Left),
            Self::Right(r) => r.state().map(Either::Right, Either::Right),
        }
    }

    fn client_keys(&self) -> impl Iterator<Item = Self::ClientKey> + '_ {
        match self {
            Self::Left(l) => Either::Left(l.client_keys().map(Either::Left)),
            Self::Right(r) => Either::Right(r.client_keys().map(Either::Right)),
        }
        .into_iter()
    }

    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ServerEvent<Self>> {
        match self {
            Self::Left(l) => Either::Left(
                l.poll(delta_time)
                    .map(|event| event.map(Either::Left, Either::Left, Either::Left)),
            ),
            Self::Right(r) => Either::Right(
                r.poll(delta_time)
                    .map(|event| event.map(Either::Right, Either::Right, Either::Right)),
            ),
        }
        .into_iter()
    }

    fn send(
        &mut self,
        client_key: impl Borrow<Self::ClientKey>,
        msg: impl Into<Bytes>,
        lane: impl Into<LaneIndex>,
    ) -> Result<Self::MessageKey, Self::SendError> {
        let client_key = client_key.borrow();
        match (self, client_key) {
            (Self::Left(l), Either::Left(client_key)) => l
                .send(client_key, msg, lane)
                .map(Either::Left)
                .map_err(Either::Left),
            (Self::Right(r), Either::Right(client_key)) => r
                .send(client_key, msg, lane)
                .map(Either::Right)
                .map_err(Either::Right),
        }
    }

    fn flush(&mut self) {
        match self {
            Self::Left(l) => l.flush(),
            Self::Right(r) => r.flush(),
        }
    }

    fn disconnect(&mut self, client_key: impl Borrow<Self::ClientKey>, reason: impl Into<String>) {
        let client_key = client_key.borrow();
        match (self, client_key) {
            (Self::Left(l), Either::Left(client_key)) => l.disconnect(client_key, reason),
            (Self::Right(r), Either::Right(client_key)) => r.disconnect(client_key, reason),
            _ => {}
        };
    }

    fn close(&mut self, reason: impl Into<String>) {
        match self {
            Self::Left(l) => l.close(reason),
            Self::Right(r) => r.close(reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn _make<Foo: ServerTransport, Bar: ServerTransport>() {
        _assert_transport::<Either<Foo, Bar>>();
    }

    fn _assert_transport<T: ServerTransport>() {}
}
