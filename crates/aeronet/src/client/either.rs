use either::Either;
use web_time::Duration;

use super::{ClientEvent, ClientState, ClientTransport};

impl<L: ClientTransport, R: ClientTransport> ClientTransport for Either<L, R> {
    type Connecting<'this> = Either<L::Connecting<'this>, R::Connecting<'this>> where Self: 'this;

    type Connected<'this> = Either<L::Connected<'this>, R::Connected<'this>> where Self: 'this;

    type MessageKey = Either<L::MessageKey, R::MessageKey>;

    type PollError = Either<L::PollError, R::PollError>;

    type SendError = Either<L::SendError, R::SendError>;

    fn state(&self) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        match self {
            Self::Left(l) => l.state().map(Either::Left, Either::Left),
            Self::Right(r) => r.state().map(Either::Right, Either::Right),
        }
    }

    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ClientEvent<Self>> {
        match self {
            Self::Left(l) => Either::Left(
                l.poll(delta_time)
                    .map(|event| event.map(Either::Left, Either::Left)),
            ),
            Self::Right(r) => Either::Right(
                r.poll(delta_time)
                    .map(|event| event.map(Either::Right, Either::Right)),
            ),
        }
        .into_iter()
    }

    fn send(
        &mut self,
        msg: impl Into<bytes::Bytes>,
        lane: impl Into<crate::lane::LaneIndex>,
    ) -> Result<Self::MessageKey, Self::SendError> {
        match self {
            Self::Left(l) => l.send(msg, lane).map(Either::Left).map_err(Either::Left),
            Self::Right(r) => r.send(msg, lane).map(Either::Right).map_err(Either::Right),
        }
    }

    fn flush(&mut self) {
        match self {
            Self::Left(l) => l.flush(),
            Self::Right(r) => r.flush(),
        }
    }

    fn disconnect(&mut self, reason: impl Into<String>) {
        match self {
            Self::Left(l) => l.disconnect(reason),
            Self::Right(r) => r.disconnect(reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn _make<Foo: ClientTransport, Bar: ClientTransport>() {
        _assert_transport::<Either<Foo, Bar>>();
    }

    fn _assert_transport<T: ClientTransport>() {}
}
