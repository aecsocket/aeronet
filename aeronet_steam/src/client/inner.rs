use std::{marker::PhantomData, time::Instant};

use aeronet::{
    LaneKey, LaneKind, LaneProtocol, MessageState, MessageTicket, OnLane, Timeout, TransportConfig,
    TryAsBytes, TryFromBytes,
};
use steamworks::{
    networking_messages::NetworkingMessages,
    networking_types::{NetworkingIdentity, SendFlags},
    ClientManager,
};

use crate::{CHALLENGE_SIZE, DISCONNECT_TOKEN, HANDSHAKE_CHANNEL, RECV_BATCH_SIZE};

use super::{ClientEvent, ClientState, LaneError, SteamTransportError};

// states

pub struct WorkingClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    net: NetworkingMessages<ClientManager>,
    target: NetworkingIdentity,
    timeout: Timeout,
    state: WorkingState,
    _phantom_p: PhantomData<P>,
}

enum WorkingState {
    Connecting {
        challenge: [u8; CHALLENGE_SIZE],
        send_connecting: bool,
    },
    Connected,
}

impl<P> Drop for WorkingClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    fn drop(&mut self) {
        let _ = self.net.send_message_to_user(
            self.target.clone(),
            // No Nagle because this is the final transmission
            SendFlags::RELIABLE_NO_NAGLE,
            DISCONNECT_TOKEN,
            HANDSHAKE_CHANNEL,
        );
    }
}

impl<P> WorkingClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    /// Creates a new client and attempts to connect it to the given target.
    ///
    /// Before this function is called, the [`steamworks::Client`]'s session
    /// request callback must be set so that it can accept sessions. See
    /// [`NetworkingMessages::session_request_callback`]. Otherwise, all
    /// connections will fail. The [`steamworks::SingleClient`] must also be
    /// continuously polled for new callbacks.
    ///
    /// This function will block until the first handshake message is sent.
    ///
    /// # Errors
    ///
    /// If the first handshake message could not be sent, this returns an error.
    ///
    /// # Panics
    ///
    /// If the protocol `P` has more than `u32::MAX - 1` lane variants.
    pub fn new(
        steam: &steamworks::Client<ClientManager>,
        target: NetworkingIdentity,
        config: TransportConfig,
    ) -> Result<Self, SteamTransportError<P>> {
        // Each lane corresponds to a channel on the Steam networking side,
        // and a channel is a single u32, so we can only have u32::MAX lanes
        // We also reserve one channel for our own internal connection
        // maintenance stuff
        let num_lanes = P::Lane::VARIANTS.len();
        let max_lanes = u32::MAX as usize - 1;
        assert!(
            num_lanes <= max_lanes,
            "too many lanes; protocol defines {num_lanes} lines, maximum {max_lanes}"
        );

        let challenge: [u8; CHALLENGE_SIZE] = rand::random();
        let net = steam.networking_messages();
        net.send_message_to_user(
            target.clone(),
            // No Nagle because we aren't sending any more data until this
            // handshake completes
            SendFlags::RELIABLE_NO_NAGLE,
            &challenge,
            HANDSHAKE_CHANNEL,
        )
        .map_err(SteamTransportError::<P>::SendConnectRequest)?;

        let timeout = Timeout::new(config.timeout);

        Ok(Self {
            net,
            target,
            state: WorkingState::Connecting {
                challenge,
                send_connecting: true,
            },
            timeout,
            _phantom_p: PhantomData::default(),
        })
    }

    pub fn client_state(&self) -> ClientState {
        match &self.state {
            WorkingState::Connecting { .. } => ClientState::Connecting,
            WorkingState::Connected { .. } => ClientState::Connected { info: () },
        }
    }

    pub fn message_state(&self, msg: MessageTicket) -> MessageState {
        todo!()
    }

    pub fn send(&self, msg: impl Into<P::C2S>) -> Result<MessageTicket, SteamTransportError<P>> {
        let WorkingState::Connected { .. } = self.state else {
            return Err(SteamTransportError::<P>::NotConnected);
        };

        let msg: P::C2S = msg.into();
        let lane = msg.lane();
        send::<P>(&self.net, self.target.clone(), msg, lane.clone())
            .map_err(|source| SteamTransportError::<P>::OnLane { lane, source })?;

        // TODO
        Ok(MessageTicket::from_raw(0))
    }

    pub fn recv(&mut self) -> Result<Vec<ClientEvent<P>>, SteamTransportError<P>> {
        if self.timeout.timed_out() {
            return Err(SteamTransportError::<P>::TimedOut);
        }

        for msg in self.net.receive_messages_on_channel(HANDSHAKE_CHANNEL, 1) {
            if msg.data() == DISCONNECT_TOKEN {
                return Err(SteamTransportError::<P>::DisconnectedByOtherSide);
            }
        }

        match &mut self.state {
            WorkingState::Connecting {
                challenge,
                send_connecting,
            } => {
                if *send_connecting {
                    // If we've just started connecting, and we haven't sent an event
                    // out for it yet, then we will send that event and do no further
                    // processing.
                    // This means that connection will never occur immediately after
                    // calling `update`, even if it's possible, but this is important to
                    // maintain the `update` contract - if it emits an event which changes
                    // the transport state, then after the call, it will be in that state.
                    *send_connecting = false;
                    return Ok(vec![ClientEvent::Connecting]);
                }

                let payloads = self.net.receive_messages_on_channel(HANDSHAKE_CHANNEL, 1);
                let Some(payload) = payloads.get(0) else {
                    return Ok(vec![]);
                };

                if payload.data() == challenge {
                    self.state = WorkingState::Connected;
                    self.timeout.update();
                    Ok(vec![ClientEvent::Connected])
                } else {
                    return Err(SteamTransportError::<P>::InvalidHandshakeToken);
                }
            }
            WorkingState::Connected => {
                let mut events = Vec::new();

                for (index, lane) in P::Lane::VARIANTS.iter().enumerate() {
                    // This is enforced during `new`
                    let channel = u32::try_from(index).unwrap();
                    update_channel(&self.net, &mut self.timeout, channel, &mut events).map_err(
                        |source| SteamTransportError::<P>::OnLane {
                            lane: lane.clone(),
                            source,
                        },
                    )?;
                }

                Ok(events)
            }
        }
    }
}

fn send<P>(
    net: &NetworkingMessages<ClientManager>,
    target: NetworkingIdentity,
    msg: P::C2S,
    lane: P::Lane,
) -> Result<(), LaneError<P>>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    let payload = msg.try_as_bytes().map_err(LaneError::<P>::Serialize)?;
    let payload: &[u8] = payload.as_ref();

    // https://partner.steamgames.com/doc/api/steamnetworkingtypes#message_sending_flags
    let send_type = match lane.kind() {
        LaneKind::UnreliableUnordered => SendFlags::UNRELIABLE,
        // TODO: UnreliableOrdered also uses reliable delivery, because it ensures ordering
        // This needs to be changed
        LaneKind::UnreliableOrdered | LaneKind::ReliableUnordered | LaneKind::ReliableOrdered => {
            SendFlags::RELIABLE
        }
    };
    // This is enforced during `new`
    let channel = u32::try_from(lane.variant()).unwrap();

    net.send_message_to_user(target, send_type, payload, channel)
        .map_err(LaneError::<P>::Send)
}

fn update_channel<P>(
    net: &NetworkingMessages<ClientManager>,
    timeout: &mut Timeout,
    channel: u32,
    events: &mut Vec<ClientEvent<P>>,
) -> Result<(), LaneError<P>>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    let payloads = net.receive_messages_on_channel(channel, RECV_BATCH_SIZE);
    if !payloads.is_empty() {
        timeout.update();
    }

    for payload in payloads {
        let msg = P::S2C::try_from_bytes(payload.data()).map_err(LaneError::<P>::Deserialize)?;
        let at = Instant::now();
        events.push(ClientEvent::Recv { msg, at })
    }

    Ok(())
}
