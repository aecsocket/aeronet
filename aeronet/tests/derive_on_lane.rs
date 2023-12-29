use aeronet::{LaneKey, OnLane};

#[derive(Debug, Clone, LaneKey)]
enum MyLane {
    #[lane_kind(UnreliableUnordered)]
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

fn assert_on_lane<T: OnLane>() {}

#[test]
fn test() {
    assert_on_lane::<MyMsg>();
}
