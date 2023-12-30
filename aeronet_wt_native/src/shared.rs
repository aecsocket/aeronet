use aeronet::{OnLane, TryAsBytes, TryFromBytes, LaneProtocol};
use wtransport::Connection;

use crate::LaneError;

async fn make_stream<P, const OPENS: bool>(conn: &Connection) -> Result<(), LaneError<P>>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    let (send_stream, recv_stream) = if OPENS {
        conn.open_bi()
            .await
            .map_err(LaneError::OpenStream)?
            .await
            .map_err(LaneError::OpeningStream)?
    } else {
        conn.accept_bi()
            .await
            .map_err(LaneError::AcceptStream)?
    };

    Ok(())
}
