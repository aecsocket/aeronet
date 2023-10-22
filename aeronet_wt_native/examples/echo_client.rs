use aeronet_wt_stream::{OnStream, Stream};

// config

#[derive(Debug, Clone, Copy, Hash, Stream)]
pub enum AppStream {
    #[stream(Datagram)]
    LowPriority,
    #[stream(Bi)]
    HighPriority,
}

#[derive(Debug, Clone, OnStream)]
#[on_stream(AppStream)]
pub enum C2S {
    #[on_stream(LowPriority)]
    Move(f32),
    #[on_stream(HighPriority)]
    Shoot,
    #[on_stream(HighPriority)]
    Chat { msg: String },
}

fn main() {
    let x = C2S::Move(4.0).on_stream().kind();
}
