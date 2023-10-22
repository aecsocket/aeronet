use aeronet_wt_stream::{OnStream, Streams};

// config

#[derive(Debug, Clone, Copy, Hash, Streams)]
pub enum AppStream {
    #[stream_kind(Datagram)]
    LowPriority,
    #[stream_kind(Bi)]
    HighPriority,
    #[stream_kind(Bi)]
    HighPriority2,
}

#[derive(Debug, Clone, OnStream)]
#[stream_type(AppStream)]
pub enum C2S {
    #[stream_variant(LowPriority)]
    Move(f32),
    #[stream_variant(HighPriority)]
    Shoot,
    #[stream_variant(HighPriority)]
    Chat { msg: String },
}

#[derive(OnStream)]
#[stream_type(AppStream)]
#[stream_variant(LowPriority)]
pub struct Message(pub String);

fn main() {
    let _ = C2S::Move(4.0).on_stream().stream_id();
}
