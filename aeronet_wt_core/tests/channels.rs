use aeronet_wt_core::*;

#[derive(Channels)]
#[channel_kind(Datagram)]
pub struct ChannelA;

#[derive(Channels)]
pub enum ChannelB {
    #[channel_kind(Datagram)]
    LowPriority,
    #[channel_kind(Stream)]
    HighPriority,
}

#[derive(Channels)]
pub enum ChannelC {
    #[channel_kind(Stream)]
    Stream1,
    #[channel_kind(Stream)]
    Stream2,
    #[channel_kind(Stream)]
    Stream3,
}

fn assert_is_channels<T: Channels>() {}

#[test]
fn test_compile() {
    assert_is_channels::<ChannelA>();
    assert_is_channels::<ChannelB>();
}

#[test]
fn test_num_streams() {
    assert_eq!(0, ChannelA::NUM_STREAMS);
    assert_eq!(1, ChannelB::NUM_STREAMS);
    assert_eq!(3, ChannelC::NUM_STREAMS);
}

#[test]
fn test_channel_id() {
    assert_eq!(ChannelId::Datagram, ChannelA.channel_id());

    assert_eq!(ChannelId::Datagram, ChannelB::LowPriority.channel_id());
    assert_eq!(ChannelId::Stream(0), ChannelB::HighPriority.channel_id());

    assert_eq!(ChannelId::Stream(0), ChannelC::Stream1.channel_id());
    assert_eq!(ChannelId::Stream(1), ChannelC::Stream2.channel_id());
    assert_eq!(ChannelId::Stream(2), ChannelC::Stream3.channel_id());
}
