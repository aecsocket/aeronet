use aeronet::{LaneKey, LaneProtocol, TryAsBytes, TryFromBytes};
use steamworks::{networking_sockets::{NetConnection, NetworkingSockets}, Manager};

use crate::{SteamTransportError, ConnectionInfo};

// https://partner.steamgames.com/doc/api/ISteamNetworkingSockets
// "The max number of lanes on Steam is 255, which is a very large number and
// not recommended!"
fn num_lanes<P>() -> u8
where
    P: LaneProtocol,
{
    u8::try_from(P::Lane::VARIANTS.len()).expect("there should be less than 256 lanes")
}

pub(super) fn assert_valid_protocol<P>()
where
    P: LaneProtocol,
{
    let _ = num_lanes::<P>();
}

pub(super) fn configure_lanes<P, S, R, M>(
    socks: &NetworkingSockets<M>,
    conn: &NetConnection<M>,
) -> Result<(), SteamTransportError<S, R>>
where
    P: LaneProtocol,
    S: TryAsBytes,
    R: TryFromBytes,
    M: Manager + Send + Sync + 'static,
{
    let num_lanes = num_lanes::<P>();
    let priorities = P::Lane::VARIANTS
        .iter()
        .map(|lane| lane.priority())
        .collect::<Vec<_>>();
    let weights = P::Lane::VARIANTS.iter().map(|_| 0).collect::<Vec<_>>();

    let num_lanes = i32::from(num_lanes);
    socks
        .configure_connection_lanes(&conn, num_lanes, &priorities, &weights)
        .map_err(SteamTransportError::<S, R>::ConfigureLanes)?;

    Ok(())
}

pub(super) fn recv_all<P, S, R, M>(
    conn: &mut NetConnection<M>,
    info: &mut ConnectionInfo,
) -> (Vec<R>, Result<(), SteamTransportError<S, R>>)
where
    P: LaneProtocol,
    S: TryAsBytes,
    R: TryFromBytes,
    M: Manager + Send + Sync + 'static,
{
    let mut msgs = Vec::new();
    loop {
        let buf = conn.receive_messages(64).unwrap_or_default();
        if buf.is_empty() {
            break;
        }

        for msg in buf {
            let bytes = msg.data();
            let msg = match R::try_from_bytes(bytes).map_err(SteamTransportError::<S, R>::Deserialize) {
                Ok(msg) => msg,
                Err(err) => return (msgs, Err(err)),
            };

            info.msgs_recv += 1;
            info.bytes_recv += bytes.len();
            msgs.push(msg);
        }
    }

    (msgs, Ok(()))
}
