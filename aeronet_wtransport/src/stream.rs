#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamC2S(pub(crate) StreamKind);

#[derive()]
pub struct StreamS2C(pub(crate) StreamKind);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum StreamKind {
    Datagram,
    Bi(usize),
    Uni(usize),
}

#[derive(Debug, Clone, Default)]
pub struct Streams {
    pub(crate) bi: usize,
    pub(crate) c2s: usize,
    pub(crate) s2c: usize,
}

impl Streams {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn c2s_datagram(&self) -> StreamC2S {
        StreamC2S(StreamKind::Datagram)
    }

    pub fn s2c_datagram(&self) -> StreamS2C {
        StreamS2C(StreamKind::Datagram)
    }

    pub fn c2s_bi(&mut self) -> StreamC2S {
        let index = self.bi;
        self.bi += 1;
        StreamC2S(StreamKind::Bi(index))
    }

    pub fn s2c_bi(&mut self) -> StreamS2C {
        let index = self.bi;
        self.bi += 1;
        StreamS2C(StreamKind::Bi(index))
    }

    pub fn c2s_uni(&mut self) -> StreamC2S {
        let index = self.c2s;
        self.c2s += 1;
        StreamC2S(StreamKind::Uni(index))
    }

    pub fn s2c_uni(&mut self) -> StreamS2C {
        let index = self.s2c;
        self.s2c += 1;
        StreamS2C(StreamKind::Uni(index))
    }
}
