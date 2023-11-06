use aeronet_wt_core::*;

#[derive(Debug, PartialEq, Channels)]
pub enum TestChannel {
    #[channel_kind(Datagram)]
    LowPriority,
    #[channel_kind(Datagram)]
    HighPriority,
}

#[derive(OnChannel)]
#[channel_type(TestChannel)]
pub enum MessageA {
    #[on_channel(TestChannel::LowPriority)]
    Move(i32),
    #[on_channel(TestChannel::HighPriority)]
    Shoot,
}

#[test]
fn test_channel() {
    assert_eq!(TestChannel::LowPriority, MessageA::Move(0).channel());
    assert_eq!(TestChannel::LowPriority, MessageA::Move(1).channel());
    assert_eq!(TestChannel::HighPriority, MessageA::Shoot.channel());
}
