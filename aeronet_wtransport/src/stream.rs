#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum StreamKind {
    Datagram,
    Bi(usize),
    Uni(usize),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Streams {
    pub(crate) bi: usize,
    pub(crate) c2s: usize,
    pub(crate) s2c: usize,
}

impl Streams {
    pub(crate) fn datagram(&self) -> StreamKind {
        StreamKind::Datagram
    }

    pub(crate) fn bi(&mut self) -> StreamKind {
        let index = self.bi;
        self.bi += 1;
        StreamKind::Bi(index)
    }

    pub(crate) fn uni_c2s(&mut self) -> StreamKind {
        let index = self.c2s;
        self.c2s += 1;
        StreamKind::Uni(index)
    }

    pub(crate) fn uni_s2c(&mut self) -> StreamKind {
        let index = self.s2c;
        self.s2c += 1;
        StreamKind::Uni(index)
    }
}
