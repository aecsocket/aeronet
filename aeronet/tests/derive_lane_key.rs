use aeronet::lane::{LaneIndex, LaneKey};

#[derive(Debug, Clone, Copy, LaneKey)]
#[lane_kind(UnreliableUnordered)]
#[drop_after(Duration::from_secs(3))]
#[resend_after(Duration::from_millis(100))]
#[ack_timeout(Duration::from_secs(30))]
struct MyLaneStruct;

#[derive(Debug, Clone, Copy, LaneKey)]
enum MyLaneEnum {
    #[lane_kind(UnreliableUnordered)]
    Variant1,
    #[lane_kind(ReliableOrdered)]
    #[drop_after(Duration::from_secs(3))]
    #[resend_after(Duration::from_millis(100))]
    #[ack_timeout(Duration::from_secs(30))]
    Variant2,
}

#[test]
fn assert_types() {
    fn assert_lane_key<T: LaneKey>() {}

    assert_lane_key::<MyLaneStruct>();
    assert_lane_key::<MyLaneEnum>();
}

#[test]
fn lane_index() {
    assert_eq!(LaneIndex::from_raw(0), MyLaneEnum::Variant1.lane_index());
    assert_eq!(LaneIndex::from_raw(1), MyLaneEnum::Variant2.lane_index());
}
