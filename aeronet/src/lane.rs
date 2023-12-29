use std::fmt::Debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaneKind {
    UnreliableUnordered,
    UnreliableOrdered,
    ReliableUnordered,
    ReliableOrdered,
}

pub trait LaneKey: Send + Sync + Debug + Clone + 'static {
    const VARIANTS: &'static [Self];

    fn variant(&self) -> usize;

    fn kind(&self) -> LaneKind;
}

pub trait OnLane {
    type Lane: LaneKey;

    fn lane(&self) -> Self::Lane;
}
