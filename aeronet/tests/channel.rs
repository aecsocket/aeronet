use aeronet::{ChannelKey, OnChannel};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ChannelKey)]
#[channel_kind(Unreliable)]
struct AppChannel1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ChannelKey)]
enum AppChannel2 {
    #[channel_kind(Unreliable)]
    LowPriority,
    #[channel_kind(ReliableUnordered)]
    HighPriority,
}

#[derive(Debug, Clone, OnChannel)]
#[channel_type(AppChannel1)]
#[on_channel(AppChannel1)]
struct AppMessage1;

#[derive(Debug, Clone, OnChannel)]
#[channel_type(AppChannel2)]
enum AppMessage2 {
    #[on_channel(AppChannel2::LowPriority)]
    Move(f32),
    #[on_channel(AppChannel2::HighPriority)]
    Chat(String),
}

#[test]
fn derive_on_struct() {
    let message = AppMessage1;
    assert_eq!(AppChannel1, message.channel());
}

#[test]
fn derive_on_enum() {
    let message = AppMessage2::Move(1.0);
    assert_eq!(AppChannel2::LowPriority, message.channel());

    let message = AppMessage2::Chat("a".into());
    assert_eq!(AppChannel2::HighPriority, message.channel());
}
