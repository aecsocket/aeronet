use js_sys::Array;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, Url, Worker};

use crate::{bindings::WebTransport, WebTransportError, WebTransportOptions};

pub struct WebTransportClient {
    transport: WebTransport,
    worker: Worker,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum WebTransportClientError {
    #[error("failed to create transport")]
    CreateTransport(#[source] WebTransportError),
    #[error("failed to create worker")]
    CreateWorker,
}

impl WebTransportClient {
    pub async fn new(
        url: impl AsRef<str>,
        options: WebTransportOptions,
    ) -> Result<Self, WebTransportClientError> {
        let url = url.as_ref();
        let worker = create_worker().map_err(|_| WebTransportClientError::CreateWorker)?;
        let transport = create_transport(url, options)
            .map_err(|err| WebTransportClientError::CreateTransport(err))?;
        let _ = JsFuture::from(transport.ready()).await;

        Ok(Self { transport, worker })
    }
}

impl Drop for WebTransportClient {
    fn drop(&mut self) {
        self.transport.close();
        self.worker.terminate();
    }
}

fn create_transport(
    url: &str,
    options: WebTransportOptions,
) -> Result<WebTransport, WebTransportError> {
    let options = options.as_js();
    WebTransport::new_with_options(url, &options).map_err(|js| WebTransportError::from_js(js))
}

const WORKER_SCRIPT: &str = "
function wait(ms) {
    return new Promise(res => setTimeout(res, ms));
}

async function read() {
    while (true) {
        await wait(100);
    }
}

read();
";

fn create_worker() -> Result<Worker, JsValue> {
    let script = Array::new();
    script.push(&JsValue::from(WORKER_SCRIPT));
    let blob = Blob::new_with_str_sequence(&script)?;
    let script_url = Url::create_object_url_with_blob(&blob)?;
    let worker = Worker::new(&script_url)?;
    Ok(worker)
}
