use aeronet::{LaneProtocol, TryAsBytes, TryFromBytes, LaneKey};
use steamworks::networking_sockets::{NetworkingSockets, NetConnection};

use crate::SteamTransportError;

// https://partner.steamgames.com/doc/api/ISteamNetworkingSockets
// "The max number of lanes on Steam is 255, which is a very large number and not recommended!"
fn num_lanes<P: LaneProtocol>() -> u8 {
    u8::try_from(P::Lane::VARIANTS.len()).expect("there should be less than 256 lanes")
}

pub(super) fn assert_valid_protocol<P: LaneProtocol>() {
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
    M: 'static,
{
    let num_lanes = num_lanes::<P>();
    let priorities = P::Lane::VARIANTS
        .iter()
        .map(|lane| lane.priority())
        .collect::<Vec<_>>();
    let weights = P::Lane::VARIANTS.iter().map(|_| 0).collect::<Vec<_>>();

    socks
        .configure_connection_lanes(&conn, i32::from(num_lanes), &priorities, &weights)
        .map_err(SteamTransportError::<S, R>::ConfigureLanes)?;

    Ok(())
}
