use std::time::Duration;

use steamworks::networking_types::{NetworkingConfigEntry, NetworkingConfigValue};

#[derive(Debug, Clone)]
pub struct SteamSessionConfig {
    pub fake_packet_loss_send: f32,
    pub fake_packet_loss_recv: f32,
    pub fake_packet_lag_send: Duration,
    pub fake_packet_lag_recv: Duration,
    pub fake_packet_reorder_send: f32,
    pub fake_packet_reorder_recv: f32,
    pub fake_packet_reorder_time: Duration,
    pub fake_packet_dup_send: f32,
    pub fake_packet_dup_recv: f32,
    pub fake_packet_dup_time_max: Duration,
    pub timeout_initial: Duration,
    pub timeout_connected: Duration,
    pub send_buffer_size: usize,
    pub send_rate_min: usize,
    pub send_rate_max: usize,
    pub mtu_packet_size: usize,
    pub symmetric_connect: bool,
    pub local_virtual_port: i32,
}

impl Default for SteamSessionConfig {
    fn default() -> Self {
        // https://github.com/ValveSoftware/GameNetworkingSockets/blob/62b395172f157ca4f01eea3387d1131400f8d604/src/steamnetworkingsockets/clientlib/csteamnetworkingsockets.cpp#L43
        Self {
            fake_packet_loss_send: 0.0,
            fake_packet_loss_recv: 0.0,
            fake_packet_lag_send: Duration::ZERO,
            fake_packet_lag_recv: Duration::ZERO,
            fake_packet_reorder_send: 0.0,
            fake_packet_reorder_recv: 0.0,
            fake_packet_reorder_time: Duration::from_millis(15),
            fake_packet_dup_send: 0.0,
            fake_packet_dup_recv: 0.0,
            fake_packet_dup_time_max: Duration::from_millis(10),
            timeout_initial: Duration::from_millis(10_000),
            timeout_connected: Duration::from_millis(10_000),
            send_buffer_size: 512 * 1024,
            send_rate_min: 256 * 1024,
            send_rate_max: 256 * 1024,
            mtu_packet_size: 1300,
            symmetric_connect: false,
            local_virtual_port: -1,
        }
    }
}

impl SteamSessionConfig {
    pub fn to_options(&self) -> Vec<NetworkingConfigEntry> {
        use NetworkingConfigEntry as Entry;
        use NetworkingConfigValue as Key;

        vec![
            Entry::new_float(Key::FakePacketLossSend, self.fake_packet_loss_send * 100.0),
            Entry::new_float(Key::FakePacketLossRecv, self.fake_packet_loss_recv * 100.0),
            Entry::new_int32(
                Key::FakePacketLagSend,
                duration_to_ms(self.fake_packet_lag_send),
            ),
            Entry::new_int32(
                Key::FakePacketLagRecv,
                duration_to_ms(self.fake_packet_lag_recv),
            ),
            Entry::new_float(
                Key::FakePacketReorderSend,
                self.fake_packet_reorder_send * 100.0,
            ),
            Entry::new_float(
                Key::FakePacketReorderRecv,
                self.fake_packet_reorder_recv * 100.0,
            ),
            Entry::new_int32(
                Key::FakePacketReorderTime,
                duration_to_ms(self.fake_packet_reorder_time),
            ),
            Entry::new_float(Key::FakePacketDupSend, self.fake_packet_dup_send * 100.0),
            Entry::new_float(Key::FakePacketDupRecv, self.fake_packet_dup_recv * 100.0),
            Entry::new_int32(
                Key::FakePacketDupTimeMax,
                duration_to_ms(self.fake_packet_dup_time_max),
            ),
            Entry::new_int32(Key::TimeoutInitial, duration_to_ms(self.timeout_initial)),
            Entry::new_int32(
                Key::TimeoutConnected,
                duration_to_ms(self.timeout_connected),
            ),
            Entry::new_int32(Key::SendBufferSize, usize_to_i32(self.send_buffer_size)),
            Entry::new_int32(Key::SendRateMin, usize_to_i32(self.send_rate_min)),
            Entry::new_int32(Key::SendRateMax, usize_to_i32(self.send_rate_max)),
            Entry::new_int32(Key::MTUPacketSize, usize_to_i32(self.mtu_packet_size)),
            Entry::new_int32(Key::SymmetricConnect, self.symmetric_connect as i32),
            Entry::new_int32(Key::LocalVirtualPort, self.local_virtual_port),
        ]
    }
}

fn u128_to_i32(n: u128) -> i32 {
    n.min(i32::MAX as u128) as i32
}

fn usize_to_i32(n: usize) -> i32 {
    n.min(i32::MAX as usize) as i32
}

fn duration_to_ms(duration: Duration) -> i32 {
    u128_to_i32(duration.as_millis())
}
