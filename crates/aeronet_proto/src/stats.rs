//! Allows drawing network statistics in an [`egui`] window.

use size::Size;
use web_time::Instant;

use crate::session::Session;

pub fn draw(ctx: &mut egui::Context, session: &Session) {
    let now = Instant::now();
    egui::Window::new("Network Stats").show(ctx, |ui| {
        ui.horizontal(|ui| {
            egui::Grid::new("labels").num_columns(2).show(ui, |ui| {
                ui.label("time");
                ui.label(format!("{:.1?}", now - session.connected_at()));
                ui.end_row();

                ui.label("tx/rx");
                ui.label(format!(
                    "{} / {}",
                    Size::from_bytes(session.bytes_sent()),
                    Size::from_bytes(session.bytes_recv())
                ));
                ui.end_row();

                ui.label("rtt");
                ui.label(format!("{:.1?}", session.rtt().get()));
                ui.end_row();

                ui.label("mem");
                ui.label(format!("{}", Size::from_bytes(session.memory_usage())));
                ui.end_row();

                // ui.label("..sent_msgs");
                // ui.label(format!("{}", Size::from_bytes(session.sent_msgs_mem())));
                // ui.end_row();

                // ui.label("..flushed_packets");
                // ui.label(format!(
                //     "{}",
                //     Size::from_bytes(session.flushed_packets_mem())
                // ));
                // ui.end_row();

                // ui.label("..recv_lanes");
                // ui.label(format!("{}", Size::from_bytes(session.recv_lanes_mem())));
                // ui.end_row();

                // ui.label("..recv_frags");
                // ui.label(format!("{}", Size::from_bytes(session.recv_frags_mem())));
                // ui.end_row();
            })
        })
    });
}
