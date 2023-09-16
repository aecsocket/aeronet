#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamId(pub(crate) usize);

impl StreamId {
    pub fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stream {
    Datagram,
    Bi(StreamId),
    Uni(StreamId),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Streams {
    pub(crate) bi: usize,
    pub(crate) c2s: usize,
    pub(crate) s2c: usize,
}

impl Streams {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_bi(&mut self) -> StreamId {
        let id = StreamId(self.bi);
        self.bi += 1;
        id
    }

    pub fn add_c2s(&mut self) -> StreamId {
        let id = StreamId(self.c2s);
        self.c2s += 1;
        id
    }

    pub fn add_s2c(&mut self) -> StreamId {
        let id = StreamId(self.s2c);
        self.s2c += 1;
        id
    }
}
