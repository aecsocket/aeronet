use aeronet::lane::LaneKey;

#[derive(Debug, Clone, Copy, LaneKey)]
#[lane_kind(UnreliableUnsequenced)]
#[drop_after(Duration::from_secs(3))]
#[resend_after(Duration::from_millis(100))]
#[ack_timeout(Duration::from_secs(30))]
struct MyLaneStruct;

#[derive(Debug, Clone, Copy, LaneKey)]
enum MyLaneEnum {
    #[lane_kind(UnreliableUnsequenced)]
    Variant1,
    #[lane_kind(ReliableOrdered)]
    #[drop_after(Duration::from_secs(3))]
    #[resend_after(Duration::from_millis(100))]
    #[ack_timeout(Duration::from_secs(30))]
    Variant2,
}

#[test]
fn test() {
    fn assert_lane_key<T: LaneKey>() {}

    assert_lane_key::<MyLaneStruct>();
    assert_lane_key::<MyLaneEnum>();
}
