use std::ops::{Deref, DerefMut};

use aeronet::{ChannelProtocol, OnChannel, TryAsBytes, TryFromBytes};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::DomException;

use crate::{bindings, WebTransportConfig, WebTransportError};

pub fn err_msg(js: JsValue) -> String {
    match js.dyn_ref::<DomException>() {
        Some(err) => err.message(),
        None => "<unknown>".to_owned(),
    }
}

pub struct WebTransport(bindings::WebTransport);

impl WebTransport {
    pub fn new<P>(
        config: WebTransportConfig,
        url: impl AsRef<str>,
    ) -> Result<Self, WebTransportError<P>>
    where
        P: ChannelProtocol,
        P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
        P::S2C: TryFromBytes,
    {
        let url = url.as_ref();
        let options = bindings::WebTransportOptions::from(config);
        match bindings::WebTransport::new_with_options(url, &options) {
            Ok(wt) => Ok(Self(wt)),
            Err(js) => Err(WebTransportError::CreateClient(err_msg(js))),
        }
    }
}

impl Drop for WebTransport {
    fn drop(&mut self) {
        self.0.close();
    }
}

impl Deref for WebTransport {
    type Target = bindings::WebTransport;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for WebTransport {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
