pub use aeronet_wt_stream_derive::{OnStream, Stream};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamKind {
    Datagram,
    Uni,
    Bi,
}

pub trait Stream {
    fn kind(&self) -> StreamKind;
}

pub trait OnStream<S> {
    fn on_stream(&self) -> S;
}
