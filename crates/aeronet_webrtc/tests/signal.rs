#![expect(missing_docs, reason = "testing")]

use {
    aeronet_webrtc::{
        IceCandidate, SessionDescription, SessionDescriptionType, Signal, SignalData,
    },
    serde_json::json,
};

#[test]
fn signal_wire_shape_contains_only_routing_and_protocol_data() {
    let candidate = Signal {
        connection_id: "opaque/routing-key".to_owned(),
        data: SignalData::IceCandidate(IceCandidate {
            candidate: "candidate:1 1 udp 1 192.0.2.1 5000 typ host".to_owned(),
            sdp_mid: Some("0".to_owned()),
            sdp_m_line_index: Some(0),
            username_fragment: Some("fragment".to_owned()),
        }),
    };

    assert_eq!(
        serde_json::to_value(&candidate).expect("serialize candidate"),
        json!({
            "connection_id": "opaque/routing-key",
            "type": "iceCandidate",
            "value": {
                "candidate": "candidate:1 1 udp 1 192.0.2.1 5000 typ host",
                "sdpMid": "0",
                "sdpMLineIndex": 0,
                "usernameFragment": "fragment"
            }
        })
    );

    for (kind, wire_kind) in [
        (SessionDescriptionType::Offer, "offer"),
        (SessionDescriptionType::Answer, "answer"),
    ] {
        let description = Signal {
            connection_id: "description".to_owned(),
            data: SignalData::SessionDescription(SessionDescription {
                kind,
                sdp: "v=0\r\n".to_owned(),
            }),
        };
        assert_eq!(
            serde_json::to_value(description).expect("serialize description"),
            json!({
                "connection_id": "description",
                "type": "sessionDescription",
                "value": { "kind": wire_kind, "sdp": "v=0\r\n" }
            })
        );
    }

    assert_eq!(
        serde_json::to_value(Signal {
            connection_id: "complete".to_owned(),
            data: SignalData::EndOfCandidates,
        })
        .expect("serialize gathering completion"),
        json!({ "connection_id": "complete", "type": "endOfCandidates" })
    );
}
