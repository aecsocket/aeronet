//! Allows drawing network statistics in an [`egui`] window.

use std::{hash::Hash, ops::RangeInclusive};

use egui::{epaint::Hsva, Align, Color32, Layout, WidgetText};
use egui_plot::{
    log_grid_spacer, uniform_grid_spacer, AxisHints, Corner, GridMark, Legend, Line, Placement,
    Plot,
};
use ringbuf::traits::{Consumer, Observer};
use size_format::{BinaryPrefixes, PointSeparated, SizeFormatter};
use web_time::Instant;

use crate::{session::Session, stats::SessionStats};

/// Allows visualizing the samples stored in a [`SessionStats`] by drawing an
/// [`egui`] window with plots and text.
///
/// If writing a client app using Bevy, you can use this together with
/// `ClientSessionStatsPlugin` to easily visualize network statistics.
/// Use this as a resource in your app, and draw the visualizer using
/// `bevy_egui` by defining a system like:
///
/// ```rust,ignore
/// fn draw_stats(
///     mut egui: EguiContexts,
///     client: Res<MyClient>,
///     stats: Res<ClientSessionStats<MyClient>>,
///     mut stats_visualizer: ResMut<SessionStatsVisualizer>,
/// ) {
///     if let ClientState::Connected(client) = client.state() {
///         stats_visualizer.draw(egui.ctx_mut(), &client.session, &*stats);
///     }
/// }
/// ```
#[derive(Debug, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct SessionStatsVisualizer {
    /// Whether to draw the RTT graph.
    pub show_rtt: bool,
    /// Whether to draw the memory usage graph.
    pub show_mem: bool,
    /// Whether to draw the bytes sent/received per second graph.
    pub show_tx_rx: bool,
    /// Whether to draw the packet loss graph.
    pub show_loss: bool, // . .. .. ._
}

impl Default for SessionStatsVisualizer {
    fn default() -> Self {
        Self {
            show_rtt: true,
            show_mem: true,
            show_tx_rx: true,
            show_loss: false,
        }
    }
}

const MAIN_COLOR: Color32 = Color32::GRAY;
const FAINT_COLOR: Color32 = Color32::DARK_GRAY;
const IN_COLOR: Hsva = color(0.60);
const OUT_COLOR: Hsva = color(0.04);

const fn color(h: f32) -> Hsva {
    Hsva {
        h,
        s: 0.8,
        v: 0.6,
        a: 1.0,
    }
}

impl SessionStatsVisualizer {
    /// Draws the session stats window.
    pub fn draw(&mut self, ctx: &mut egui::Context, session: &Session, stats: &SessionStats) {
        let now = Instant::now();
        egui::Window::new("Network Stats").show(ctx, |ui| {
            let samples = stats.capacity().get();
            let history = samples as f64 / stats.sample_rate() as f64;

            let mut rtt = Vec::with_capacity(samples);
            let mut crtt = Vec::with_capacity(samples);
            let mut buf_mem = Vec::with_capacity(samples);
            let mut bytes_used = Vec::with_capacity(samples);
            let mut tx = Vec::with_capacity(samples);
            let mut rx = Vec::with_capacity(samples);
            for (index, sample) in stats.iter().rev().enumerate() {
                let x = -(index as f64 / stats.sample_rate() as f64);
                rtt.push([x, sample.rtt.as_secs_f64() * 1000.0]);
                crtt.push([x, sample.conservative_rtt.as_secs_f64() * 1000.0]);
                buf_mem.push([x, sample.memory_usage as f64]);
                bytes_used.push([x, sample.bytes_used as f64]);
                tx.push([x, sample.tx as f64 * stats.sample_rate() as f64]);
                rx.push([x, sample.rx as f64 * stats.sample_rate() as f64]);
            }

            ui.horizontal(|ui| {
                if self.show_rtt {
                    plot(history, "rtt")
                        .custom_y_axes(vec![axis_hints("ms").max_digits(4)])
                        .show(ui, |ui| {
                            ui.line(Line::new(rtt).name("RTT").color(MAIN_COLOR));
                            ui.line(Line::new(crtt).name("cRTT").color(FAINT_COLOR));
                        });
                }

                if self.show_mem {
                    plot(history, "mem")
                        .y_grid_spacer(log_grid_spacer(2))
                        .custom_y_axes(vec![axis_hints("bytes")])
                        .y_axis_formatter(fmt_bytes_y_axis)
                        .show(ui, |ui| {
                            ui.line(Line::new(buf_mem).name("Buf Mem").color(MAIN_COLOR));
                            ui.line(Line::new(bytes_used).name("Bytes Used").color(FAINT_COLOR));
                        });
                }

                if self.show_tx_rx {
                    plot(history, "tx_rx")
                        .y_grid_spacer(log_grid_spacer(2))
                        .custom_y_axes(vec![axis_hints("bytes/sec")])
                        .y_axis_formatter(fmt_bytes_y_axis)
                        .show(ui, |ui| {
                            ui.line(Line::new(tx).name("Tx").color(OUT_COLOR));
                            ui.line(Line::new(rx).name("Rx").color(IN_COLOR));
                        });
                }
            });

            ui.horizontal(|ui| {
                ui.label(format!("sampled @ {} Hz", stats.sample_rate()));
                ui.separator();

                ui.label(format!("{:.1?}", now - session.connected_at()));
                ui.separator();

                ui.label(format!(
                    "{}B tx / {}B rx",
                    fmt_bytes(session.bytes_sent()),
                    fmt_bytes(session.bytes_recv())
                ));
                ui.separator();

                ui.label(format!(
                    "{}B used / {}B max",
                    fmt_bytes(session.memory_usage()),
                    fmt_bytes(session.max_memory_usage())
                ));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_enabled_ui(false, |ui| {
                        ui.checkbox(&mut self.show_loss, "Loss");
                    });
                    ui.checkbox(&mut self.show_tx_rx, "Tx/Rx");
                    ui.checkbox(&mut self.show_mem, "Mem");
                    ui.checkbox(&mut self.show_rtt, "RTT");
                });
            });
        });
    }
}

fn plot(history: f64, id_source: impl Hash) -> Plot {
    egui_plot::Plot::new(id_source)
        .height(150.0)
        .view_aspect(2.5)
        .allow_drag([true, false])
        .allow_zoom([true, false])
        .allow_scroll([true, false])
        .allow_boxed_zoom(false)
        .set_margin_fraction([0.0, 0.05].into())
        .include_x(-history)
        .include_y(0.0)
        .x_axis_label("sec")
        .x_grid_spacer(uniform_grid_spacer(|_| [10.0, 5.0, 1.0]))
        .y_axis_width(4)
        .legend(Legend::default().position(Corner::LeftTop))
}

fn axis_hints(label: impl Into<WidgetText>) -> AxisHints {
    AxisHints::new_y()
        .label(label)
        .placement(Placement::RightTop)
}

fn fmt_bytes(bytes: usize) -> String {
    format!(
        "{:.0}",
        SizeFormatter::<usize, BinaryPrefixes, PointSeparated>::new(bytes)
    )
}

fn fmt_bytes_y_axis(mark: GridMark, _max_digits: usize, _range: &RangeInclusive<f64>) -> String {
    fmt_bytes(mark.value as usize)
}