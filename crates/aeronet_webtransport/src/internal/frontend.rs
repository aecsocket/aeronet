use aeronet::{error::pretty_error, lane::LaneIndex};
use aeronet_proto::session::FatalSendError;
use bytes::Bytes;
use tracing::{debug, trace};
use web_time::{Duration, Instant};

use crate::shared::MessageKey;

use super::{ConnectionInner, InternalError};

#[derive(Debug)]
pub enum PollEvent {
    Ack { msg_key: MessageKey },
    Recv { msg: Bytes, lane: LaneIndex },
}

impl<E> ConnectionInner<E> {
    pub fn send(&mut self, msg: Bytes, lane: LaneIndex) -> Result<MessageKey, InternalError<E>> {
        let err = match self.session.send(Instant::now(), msg, lane) {
            Ok(seq) => {
                return Ok(MessageKey::from_raw(lane, seq));
            }
            Err(err) => err,
        };

        match err.narrow::<FatalSendError, _>() {
            Ok(err) => {
                self.fatal_error = Some(err.clone());
                Err(InternalError::FatalSend(err))
            }
            Err(err) => Err(InternalError::Send(err.take())),
        }
    }

    pub fn flush(&mut self) {
        let mut bytes_sent = 0usize;
        for packet in self.session.flush(Instant::now()) {
            bytes_sent = bytes_sent.saturating_add(packet.len());
            // ignore errors here, pick them up in `poll`
            let _ = self.send_msgs.unbounded_send(packet);
        }

        if bytes_sent > 0 {
            trace!(bytes_sent, "Flushed packets");
        }
    }

    pub fn poll(
        &mut self,
        delta_time: Duration,
        mut cb: impl FnMut(PollEvent),
    ) -> Result<(), InternalError<E>> {
        if let Some(err) = self
            .recv_err
            .try_recv()
            .map_err(|_| InternalError::BackendClosed)?
        {
            return Err(InternalError::Spec(err));
        }

        while let Ok(Some(meta)) = self.recv_meta.try_next() {
            #[cfg(not(target_family = "wasm"))]
            {
                self.remote_addr = meta.remote_addr;
                self.raw_rtt = meta.rtt;
            }
            self.session
                .set_mtu(meta.mtu)
                .map_err(InternalError::MtuTooSmall)?;
        }

        let mut bytes_recv = 0usize;
        while let Ok(Some(packet)) = self.recv_msgs.try_next() {
            bytes_recv = bytes_recv.saturating_add(packet.len());
            let (acks, msgs) = match self.session.recv(Instant::now(), packet) {
                Ok(x) => x,
                Err(err) => {
                    debug!(
                        "Error while reading packet from server: {:#}",
                        pretty_error(&err)
                    );
                    continue;
                }
            };

            for (lane, seq) in acks {
                cb(PollEvent::Ack {
                    msg_key: MessageKey::from_raw(lane, seq),
                });
            }

            msgs.for_each_msg(|res| match res {
                Ok((msg, lane)) => {
                    cb(PollEvent::Recv { msg, lane });
                }
                Err(err) => {
                    debug!(
                        "Error while reading packet from server: {:#}",
                        pretty_error(&err)
                    );
                }
            });
        }

        self.session
            .update(delta_time)
            .map_err(InternalError::OutOfMemory)?;

        if bytes_recv > 0 {
            trace!(bytes_recv, "Received packets");
        }

        Ok(())
    }
}
