use aeronet::LaneKey;

#[derive(Debug, Clone, LaneKey)]
#[lane_kind(UnreliableUnsequenced)]
struct MyLaneStruct;

#[derive(Debug, Clone, LaneKey)]
enum MyLaneEnum {
    #[lane_kind(UnreliableUnsequenced)]
    Variant1,
    #[lane_kind(ReliableOrdered)]
    Variant2,
}

#[test]
fn test() {
    fn assert_lane_key<T: LaneKey>() {}

    assert_lane_key::<MyLaneStruct>();
    assert_lane_key::<MyLaneEnum>();
}
