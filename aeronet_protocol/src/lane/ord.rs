#[derive(Debug)]
pub struct Unsequenced;

#[derive(Debug)]
pub struct Sequenced;

#[derive(Debug)]
pub struct Unordered;

#[derive(Debug)]
pub struct Ordered;

mod private {
    pub trait Sealed {}

    impl Sealed for super::Unsequenced {}

    impl Sealed for super::Sequenced {}

    impl Sealed for super::Unordered {}

    impl Sealed for super::Ordered {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SequencingKind {
    Unsequenced,
    Sequenced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderingKind {
    Unordered,
    Sequenced,
    Ordered,
}

pub trait Sequencing: private::Sealed {
    const KIND: SequencingKind;
}

pub trait Ordering: private::Sealed {
    const KIND: OrderingKind;
}

impl Sequencing for Unsequenced {
    const KIND: SequencingKind = SequencingKind::Unsequenced;
}

impl Sequencing for Sequenced {
    const KIND: SequencingKind = SequencingKind::Sequenced;
}

impl Ordering for Sequenced {
    const KIND: OrderingKind = OrderingKind::Sequenced;
}

impl Ordering for Unordered {
    const KIND: OrderingKind = OrderingKind::Unordered;
}

impl Ordering for Ordered {
    const KIND: OrderingKind = OrderingKind::Ordered;
}
