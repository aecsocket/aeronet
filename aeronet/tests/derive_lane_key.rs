use aeronet::LaneKey;

#[derive(Debug, Clone, LaneKey)]
#[lane_kind(UnreliableUnordered)]
struct MyLaneStruct;

#[derive(Debug, Clone, LaneKey)]
enum MyLaneEnum {
    #[lane_kind(UnreliableUnordered)]
    Variant1,
    #[lane_kind(ReliableOrdered)]
    Variant2,
}

fn assert_lane_key<T: LaneKey>() {}

#[test]
fn test() {
    assert_lane_key::<MyLaneStruct>();
    assert_lane_key::<MyLaneEnum>();
}
