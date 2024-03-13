use aeronet::lane::{LaneKey, OnLane};

#[derive(Debug, Clone, Copy, LaneKey)]
enum MyLane {
    #[lane_kind(UnreliableUnordered)]
    A,
    #[lane_kind(ReliableOrdered)]
    B,
}

#[derive(OnLane)]
#[allow(dead_code)]
enum MyMsg {
    #[on_lane(MyLane::A)]
    Variant1,
    #[on_lane(MyLane::B)]
    Variant2,
}

#[test]
fn assert_types() {
    fn assert_on_lane<T: OnLane>() {}

    assert_on_lane::<MyMsg>();
}

#[test]
fn lane_index() {
    assert_eq!(MyLane::A.lane_index(), MyMsg::Variant1.lane_index());
    assert_eq!(MyLane::B.lane_index(), MyMsg::Variant2.lane_index());
}
