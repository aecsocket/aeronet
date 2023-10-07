use crate::{bindings::WebTransport, WebTransportOptions};

pub struct WebTransportClient {
    transport: WebTransport,
}

impl WebTransportClient {
    pub fn new(url: impl AsRef<str>, options: WebTransportOptions) -> Self {
        let url = url.as_ref();
        let options = options.as_js();
        let transport = WebTransport::new_with_options(url, &options);
        todo!()
    }
}
