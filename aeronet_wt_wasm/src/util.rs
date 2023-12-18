use std::ops::{Deref, DerefMut};

use aeronet::{ChannelProtocol, OnChannel, TryAsBytes, TryFromBytes};
use js_sys::Reflect;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{DomException, ReadableStreamDefaultReader, WritableStreamDefaultWriter};

use crate::{bind, WebTransportConfig, WebTransportError};

pub fn err_msg(js: &JsValue) -> String {
    match js.dyn_ref::<DomException>() {
        Some(err) => err.message(),
        None => "<unknown>".to_owned(),
    }
}

pub struct WebTransport(bind::WebTransport);

impl WebTransport {
    pub fn new<P>(
        config: &WebTransportConfig,
        url: impl AsRef<str>,
    ) -> Result<Self, WebTransportError<P>>
    where
        P: ChannelProtocol,
        P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
        P::S2C: TryFromBytes,
    {
        let url = url.as_ref();
        let options = bind::WebTransportOptions::from(config);
        match bind::WebTransport::new_with_options(url, &options) {
            Ok(wt) => Ok(Self(wt)),
            Err(js) => Err(WebTransportError::CreateClient(err_msg(&js))),
        }
    }
}

impl Drop for WebTransport {
    fn drop(&mut self) {
        self.0.close();
    }
}

impl Deref for WebTransport {
    type Target = bind::WebTransport;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for WebTransport {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub struct StreamReader(ReadableStreamDefaultReader);

impl<T> From<T> for StreamReader
where
    T: Into<JsValue>,
{
    fn from(value: T) -> Self {
        Self(ReadableStreamDefaultReader::from(value.into()))
    }
}

impl StreamReader {
    pub async fn read<T>(&self) -> Result<(T, bool), String>
    where
        T: From<JsValue>,
    {
        JsFuture::from(self.0.read())
            .await
            .map(|js| {
                let bytes = T::from(Reflect::get(&js, &JsValue::from("value")).unwrap());
                let done = Reflect::get(&js, &JsValue::from("done"))
                    .unwrap()
                    .as_bool()
                    .unwrap();
                (bytes, done)
            })
            .map_err(|js| err_msg(&js))
    }
}

pub struct StreamWriter(WritableStreamDefaultWriter);

impl<T> From<T> for StreamWriter
where
    T: Into<JsValue>,
{
    fn from(value: T) -> Self {
        Self(WritableStreamDefaultWriter::from(value.into()))
    }
}

impl StreamWriter {
    pub async fn write<T>(&self, chunk: T) -> Result<(), String>
    where
        T: Into<JsValue>,
    {
        JsFuture::from(self.0.write_with_chunk(&chunk.into()))
            .await
            .map(|_| ())
            .map_err(|js| err_msg(&js))
    }
}
