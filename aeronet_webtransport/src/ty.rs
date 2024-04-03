pub use xwt::current::{Connection, RecvStream, SendStream};

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        pub type Endpoint = xwt::current::Endpoint;
        pub type Datagram = Vec<u8>;
        // pub type Connecting = xwt::current::Connecting;
        // pub type OpenBiStream = Connection;
        // pub type OpeningBiStream = xwt_core::utils::dummy::OpeningBiStream<xwt::current::Connection>;
    } else {
        pub type Endpoint = xwt::current::Endpoint<wtransport::endpoint::endpoint_side::Client>;
        pub type Datagram = xwt::current::Datagram;
        pub type Connecting = xwt_core::utils::dummy::Connecting<wtransport::Connection>;
        pub type OpenBiStream = Connection;
        pub type OpeningBiStream = xwt::current::OpeningBiStream;
        pub type AcceptBiStream = xwt::current::Connection;
    }
}
