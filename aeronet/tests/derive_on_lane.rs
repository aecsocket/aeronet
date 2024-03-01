use aeronet::lane::{LaneKey, OnLane};

#[derive(Debug, Clone, Copy, LaneKey)]
enum MyLane {
    #[lane_kind(UnreliableUnsequenced)]
    A,
    #[lane_kind(ReliableOrdered)]
    B,
}

#[derive(OnLane)]
#[lane_type(MyLane)]
#[allow(dead_code)]
enum MyMsg {
    #[on_lane(MyLane::A)]
    Variant1,
    #[on_lane(MyLane::B)]
    Variant2,
}

#[test]
fn test() {
    fn assert_on_lane<T: OnLane>() {}

    assert_on_lane::<MyMsg>();
}
