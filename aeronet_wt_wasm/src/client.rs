use std::marker::PhantomData;

use aeronet::{
    ClientEvent, ClientTransport, Message, SessionError, TryFromBytes, TryIntoBytes,
};
use crossbeam_channel::{Receiver, Sender};
use js_sys::{Reflect, Uint8Array};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    ReadableStreamDefaultReader, WritableStreamDefaultWriter,
};

use crate::bindings::WebTransport;

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

  I LIED it doesnt work this way anymore
*/

const CHANNEL_BUF: usize = 128;

#[derive(Debug, Clone, thiserror::Error)]
pub enum WebTransportError {
    #[error("failed to create transport")]
    CreateTransport,
}

struct Inner<C2S, S2C> {
    transport: WebTransport,
    recv_events: Receiver<ClientEvent<S2C>>,
    send_events: Sender<ClientEvent<S2C>>,
    writer: WritableStreamDefaultWriter,
    events: Vec<ClientEvent<S2C>>,
    _phantom_c2s: PhantomData<C2S>,
}

impl<C2S, S2C> Drop for Inner<C2S, S2C> {
    fn drop(&mut self) {
        self.transport.close();
    }
}

pub struct WebTransportClient<C2S, S2C> {
    inner: Option<Inner<C2S, S2C>>,
}

impl<C2S, S2C> WebTransportClient<C2S, S2C>
where
    C2S: Message,
    S2C: Message + TryFromBytes,
{
    pub fn new() -> Self {
        Self {
            inner: None,
        }
    }

    pub async fn connect(
        &mut self,
        url: impl AsRef<str>,
    ) -> Result<(), WebTransportError> {
        if self.inner.is_some() {
            return Ok(());
        }

        let url = url.as_ref();
        let transport = WebTransport::new(url).map_err(|_| WebTransportError::CreateTransport)?;
        JsFuture::from(transport.ready())
            .await
            .map_err(|_| WebTransportError::CreateTransport)?;

        let (send_events, recv_events) = crossbeam_channel::bounded::<ClientEvent<S2C>>(CHANNEL_BUF);
        let reader = ReadableStreamDefaultReader::from(JsValue::from(
            transport.datagrams().readable().get_reader(),
        ));

        {
            let send_events = send_events.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = match Self::recv_from_reader(reader).await {
                    Ok(msg) => send_events.send(ClientEvent::Recv { msg }),
                    Err(reason) => send_events.send(ClientEvent::Disconnected { reason }),
                };
            });
        }

        let writer = transport.datagrams().writable().get_writer().unwrap();
        
        self.inner = Some(Inner {
            transport,
            recv_events,
            send_events,
            writer,
            events: Vec::new(),
            _phantom_c2s: PhantomData::default(),
        });
        Ok(())
    }

    async fn recv_from_reader(reader: ReadableStreamDefaultReader) -> Result<S2C, SessionError> {
        let (payload, done) = JsFuture::from(reader.read())
            .await
            .and_then(|js| {
                let value = Uint8Array::from(Reflect::get(&js, &JsValue::from("value")).unwrap());
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

impl<C2S, S2C> ClientTransport<C2S, S2C> for WebTransportClient<C2S, S2C>
where
    C2S: Message + TryIntoBytes,
    S2C: Message,
{
    // technically there is this
    // https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/getStats
    // but it has ~0 compatibility with anything (as of now)
    type Info = ();

    fn recv(&mut self) {
        let Some(Inner { recv_events, events, .. }) = self.inner else {
            return;
        };

        recv_events.try_recv().map_err(|err| match err {
            crossbeam_channel::TryRecvError::Empty => RecvError::Empty,
            _ => RecvError::Closed,
        });
    }

    fn take_events(&mut self) -> impl Iterator<Item = ClientEvent<S2C>> + '_ {
        std::iter::empty()
    }

    fn send(&mut self, msg: impl Into<C2S>) {
        let Some(Inner { writer, send_events, .. }) = self.inner else {
            return;
        };

        if let Err(reason) = (|| {
            let msg: C2S = msg.into();
            let payload = msg
                .try_into_bytes()
                .map_err(|err| SessionError::Transport(anyhow::anyhow!("send err")))?; // TODO
            let chunk = Uint8Array::new_with_length(payload.len().try_into().unwrap());
            chunk.copy_from(&payload);

            let fut = JsFuture::from(writer.write_with_chunk(&chunk.into()));
            wasm_bindgen_futures::spawn_local(async move {
                fut.await;
            });
            Ok::<_, SessionError>(())
        })() {
            // TODO just an error event maybe? or force dc?
            let _ = send_events.send(ClientEvent::Disconnected { reason });
        }
    }

    fn info(&self) -> Option<Self::Info> {
        
        todo!()
    }

    fn connected(&self) -> bool {
        todo!()
    }
}
