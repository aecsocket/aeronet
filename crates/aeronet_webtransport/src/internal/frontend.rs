use {
    super::{InternalSession, SessionError, SessionSendError},
    crate::shared::MessageKey,
    aeronet::{client::DisconnectReason, error::pretty_error, lane::LaneIndex},
    aeronet_proto::session::FatalSendError,
    bytes::Bytes,
    std::num::Saturating,
    tracing::{debug, trace},
    web_time::{Duration, Instant},
};

#[derive(Debug)]
pub enum PollEvent {
    Ack { msg_key: MessageKey },
    Recv { msg: Bytes, lane: LaneIndex },
}

impl InternalSession {
    pub fn send(&mut self, msg: Bytes, lane: LaneIndex) -> Result<MessageKey, SessionSendError> {
        let err = match self.session.send(Instant::now(), msg, lane) {
            Ok(key) => {
                return Ok(key);
            }
            Err(err) => err,
        };

        match err.narrow::<FatalSendError, _>() {
            Ok(err) => {
                self.fatal_error = Some(err.clone());
                Err(SessionSendError::Fatal(err))
            }
            Err(err) => Err(SessionSendError::Trivial(err.take())),
        }
    }

    pub fn flush(&mut self) {
        let mut bytes_sent = Saturating(0usize);
        for packet in self.session.flush(Instant::now()) {
            bytes_sent += packet.len();
            // ignore errors here, pick them up in `poll`
            let _ = self.send_msgs.unbounded_send(packet);
        }

        let bytes_sent = bytes_sent.0;
        if bytes_sent > 0 {
            trace!(bytes_sent, "Flushed packets");
        }
    }

    pub fn poll(
        &mut self,
        delta_time: Duration,
        mut cb: impl FnMut(PollEvent),
    ) -> Result<(), DisconnectReason<SessionError>> {
        while let Ok(Some(meta)) = self.recv_meta.try_next() {
            #[cfg(not(target_family = "wasm"))]
            {
                self.remote_addr = meta.remote_addr;
                self.raw_rtt = meta.rtt;
            }
            self.session
                .set_mtu(meta.mtu)
                .map_err(SessionError::MtuTooSmall)?;
        }

        let mut bytes_recv = Saturating(0usize);
        while let Ok(Some(packet)) = self.recv_msgs.try_next() {
            bytes_recv += packet.len();
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
            .map_err(SessionError::OutOfMemory)?;

        let bytes_recv = bytes_recv.0;
        if bytes_recv > 0 {
            trace!(bytes_recv, "Received packets");
        }

        Ok(())
    }
}
