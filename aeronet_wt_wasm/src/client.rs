use crate::{
    bindings::WebTransport,
    WebTransportOptions, WebTransportError,
};

pub struct WebTransportClient {
    transport: WebTransport,
}

impl WebTransportClient {
    pub fn new(
        url: impl AsRef<str>,
        options: WebTransportOptions,
    ) -> Result<Self, WebTransportError> {
        let url = url.as_ref();
        let options = options.as_js();
        let transport = WebTransport::new_with_options(url, &options)
            .map_err(|js| WebTransportError::from_js(js))?;
        todo!()
    }
}

impl Drop for WebTransportClient {
    fn drop(&mut self) {
        self.transport.close();
    }
}
