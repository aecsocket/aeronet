use std::{marker::PhantomData, sync::mpsc};

use aeronet::{ClientEvent, ClientTransport, Message, RecvError, SessionError, TryFromBytes};
use js_sys::{Array, Reflect, Uint8Array};
use wasm_bindgen::{prelude::Closure, JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, MessageEvent, ReadableStreamDefaultReader, Url, Worker};

use crate::{bindings::WebTransport, WebTransportErrorUnused, WebTransportOptions};

/*
implementation notes:
* use our WebTransport bindings for wasm
* spawn a web worker which:
  * receives a WebTransportDatagramDuplexStream.readable from rust
  * gets the readable.getReader() and saves it into `reader`
  * in an infinite loop, reads from `reader`
  * posts the reader data back to rust
  * this is because we can't use getReader().read() from rust yet
* on the rust side:
  * uhhhh idk?
*/

pub struct WebTransportClient<C2S, S2C> {
    transport: WebTransport,
    worker: Worker,
    recv_s2c: mpsc::Receiver<ClientEvent<S2C>>,
    _phantom_c2s: PhantomData<C2S>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum WebTransportError {
    #[error("failed to create transport")]
    CreateTransport,
    #[error("failed to create worker")]
    CreateWorker,
}

impl<C2S, S2C> WebTransportClient<C2S, S2C>
where
    C2S: Message,
    S2C: Message + TryFromBytes,
{
    pub fn new(
        url: impl AsRef<str>,
        options: WebTransportOptions,
    ) -> Result<Self, WebTransportError> {
        let url = url.as_ref();
        let transport =
            create_transport(url, options).map_err(|_| WebTransportError::CreateTransport)?;
        let worker = create_worker().map_err(|_| WebTransportError::CreateWorker)?;

        let (send_s2c, recv_s2c) = mpsc::channel::<ClientEvent<S2C>>();
        let reader = ReadableStreamDefaultReader::from(JsValue::from(
            transport.datagrams().readable().get_reader(),
        ));

        wasm_bindgen_futures::spawn_local(async move {
            let _ = match Self::recv_from_reader(reader).await {
                Ok(msg) => send_s2c.send(ClientEvent::Recv { msg }),
                Err(reason) => send_s2c.send(ClientEvent::Disconnected { reason }),
            };
        });

        Ok(Self {
            transport,
            worker,
            recv_s2c,
            _phantom_c2s: PhantomData::default(),
        })
    }

    async fn recv_from_reader(
        reader: ReadableStreamDefaultReader,
    ) -> Result<S2C, SessionError> {
        let (payload, done) = JsFuture::from(reader.read())
            .await
            .and_then(|js| {
                let value =
                    Uint8Array::from(Reflect::get(&js, &JsValue::from("value")).unwrap());
                let done = Reflect::get(&js, &JsValue::from("done"))
                    .unwrap()
                    .as_bool()
                    .unwrap();
                Ok((value, done))
            })
            .unwrap();
        if done {
            // todo turn this into its own error type
            return Err(SessionError::Transport(anyhow::anyhow!("closed")));
        }

        let payload = payload.to_vec();
        let msg = S2C::try_from_bytes(payload.as_slice())
            .map_err(|err| SessionError::Transport(err.into()))?;
        Ok(msg)
    }
}

impl<C2S, S2C> Drop for WebTransportClient<C2S, S2C> {
    fn drop(&mut self) {
        self.transport.close();
        self.worker.terminate();
    }
}

impl<C2S, S2C> ClientTransport<C2S, S2C> for WebTransportClient<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    type Info = ();

    fn recv(&mut self) -> Result<ClientEvent<S2C>, RecvError> {
        self.recv_s2c.try_recv().map_err(|err| match err {
            mpsc::TryRecvError::Empty => RecvError::Empty,
            _ => RecvError::Closed,
        })
    }

    fn send(&mut self, msg: impl Into<C2S>) {
        todo!()
    }

    fn info(&self) -> Option<Self::Info> {
        todo!()
    }

    fn connected(&self) -> bool {
        todo!()
    }
}

fn create_transport(
    url: &str,
    options: WebTransportOptions,
) -> Result<WebTransport, WebTransportErrorUnused> {
    let options = options.as_js();
    WebTransport::new_with_options(url, &options).map_err(|js| WebTransportErrorUnused::from_js(js))
}

const WORKER_SCRIPT: &str = "
let reader = null;

function sleep(ms) {
    return new Promise(res => setTimeout(res, ms));
}

self.onmessage = function(event) {
    if (event.data) {
        reader = event.data.getReader();
    }
};

async function read() {
    while (true) {
        if (reader) {
            const { value, done } = await reader.read();
            if (done) {
                break;
            }
            self.postMessage(value);
        } else {
            await sleep(100);
        }
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
