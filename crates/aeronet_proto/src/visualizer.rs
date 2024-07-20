//! Allows drawing network statistics in an [`egui`] window.

use egui_plot::{AxisHints, Corner, Legend, Line, Placement, PlotPoint, PlotPoints};
use ringbuf::traits::{Consumer, Observer};
use size::Size;
use web_time::Instant;

use crate::{session::Session, stats::SessionStats};

pub fn draw(ctx: &mut egui::Context, session: &Session, stats: &SessionStats) {
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
            });
        });

        let samples = stats.capacity().get();
        let mut rtt = Vec::with_capacity(samples);
        let mut rtt_conservative = Vec::with_capacity(samples);
        let mut memory_usage = Vec::with_capacity(samples);
        for (index, sample) in stats.iter().rev().enumerate() {
            let x = -(index as f64 / stats.update_freq() as f64);
            rtt.push([x, sample.rtt.as_secs_f64() * 1000.0]);
            rtt_conservative.push([x, sample.rtt_conservative.as_secs_f64() * 1000.0]);
            memory_usage.push([x, sample.memory_usage as f64]);
        }

        egui_plot::Plot::new("rtt")
            .allow_drag([true, false])
            .allow_zoom([true, false])
            .allow_scroll([true, false])
            .allow_boxed_zoom(false)
            .include_x(-(samples as f64 / stats.update_freq() as f64))
            .include_y(0.0)
            .view_aspect(2.5)
            .x_axis_label("delta (sec)")
            .custom_y_axes(vec![AxisHints::new_y()
                .label("rtt (ms)")
                .max_digits(4)
                .placement(Placement::RightTop)])
            .show(ui, |ui| {
                ui.line(Line::new(rtt).name("RTT"));
                ui.line(Line::new(rtt_conservative).name("cRTT"));
            });
    });
}
