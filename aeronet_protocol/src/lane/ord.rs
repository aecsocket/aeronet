#[derive(Debug)]
pub struct Unordered;

#[derive(Debug)]
pub struct Sequenced;

#[derive(Debug)]
pub struct Ordered;

mod private {
    pub trait Sealed {}

    impl Sealed for super::Unordered {}

    impl Sealed for super::Sequenced {}

    impl Sealed for super::Ordered {}
}

// sequencing

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SequencingKind {
    Unordered,
    Sequenced,
}

pub trait Sequencing: private::Sealed {
    const KIND: SequencingKind;
}

impl Sequencing for Unordered {
    const KIND: SequencingKind = SequencingKind::Unordered;
}

impl Sequencing for Sequenced {
    const KIND: SequencingKind = SequencingKind::Sequenced;
}

// ordering

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderingKind {
    Unordered,
    Sequenced,
    Ordered,
}

pub trait Ordering: private::Sealed {
    const KIND: OrderingKind;
}

impl Ordering for Unordered {
    const KIND: OrderingKind = OrderingKind::Unordered;
}

impl Ordering for Sequenced {
    const KIND: OrderingKind = OrderingKind::Sequenced;
}

impl Ordering for Ordered {
    const KIND: OrderingKind = OrderingKind::Ordered;
}
