use std::{hash::Hash, ops::RangeInclusive, time::Duration};

use bevy_app::prelude::*;
use bevy_core::Name;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::prelude::*;
use bevy_egui::{
    egui::{self, Align, Color32, Layout, WidgetText},
    EguiContexts,
};
use egui_plot::{
    log_grid_spacer, uniform_grid_spacer, AxisHints, Corner, GridMark, Legend, Line, Placement,
    Plot,
};
use itertools::Itertools;
use ringbuf::{
    traits::{Consumer, RingBuffer},
    HeapRb,
};
use size_format::{BinaryPrefixes, PointSeparated, SizeFormatter};
use web_time::Instant;

use crate::{
    io::IoStats,
    stats::{ConnectedAt, Rtt},
    util::display_name,
};

#[derive(Debug)]
pub struct SessionStatsVisualizerPlugin;

impl Plugin for SessionStatsVisualizerPlugin {
    fn build(&self, app: &mut App) {}
}

#[derive(Debug, Clone, PartialEq, Eq, Component)]
pub struct SessionStatsVisualizer {
    /// Whether to draw the RTT graph.
    pub show_rtt: bool,
    /// Whether to draw the bytes sent/received per second graph.
    pub show_rx_tx: bool,
    /// Whether to draw the packet loss graph.
    pub show_loss: bool, // . .. .. ._
    /// Color of plot line used to represent amount of incoming data.
    pub color_in: Color32,
    /// Color of plot line used to represent amount of outgoing data.
    pub color_out: Color32,
}

impl Default for SessionStatsVisualizer {
    fn default() -> Self {
        Self {
            show_rtt: true,
            show_rx_tx: true,
            show_loss: true,
            color_in: Color32::RED,
            color_out: Color32::BLUE,
        }
    }
}

pub struct SessionStatsParam<'a> {
    pub connected_at: Instant,
    pub rtt: Duration,
    pub io: IoStats,
    pub sample_rate: f64,
    pub samples: &'a SessionStats,
}

impl SessionStatsVisualizer {
    pub fn show(&mut self, ui: &mut egui::Ui, stats: &SessionStatsParam) {
        let now = Instant::now();
        let history = 15.0;

        let (rtt, crtt, rx, tx): (Vec<_>, Vec<_>, Vec<_>, Vec<_>) = stats
            .samples
            .iter()
            .rev()
            .enumerate()
            .map(|(index, sample)| {
                let x = -(index as f64 / stats.sample_rate);
                (
                    [x, (sample.rtt.as_millis() * 1000) as f64],
                    [x, (sample.conservative_rtt.as_millis() * 1000) as f64],
                    [x, sample.bytes_in_delta as f64 * stats.sample_rate],
                    [x, sample.bytes_out_delta as f64 * stats.sample_rate],
                    // [x, sample.memory_usage as f64],
                    // [x, sample.bytes_used as f64],
                    // [x, sample.loss * 100.0],
                )
            })
            .multiunzip();

        ui.horizontal(|ui| {
            let main_color = ui.visuals().text_color();
            let weak_color = ui.visuals().weak_text_color();

            if self.show_rtt {
                plot(history, "rtt")
                    .y_grid_spacer(uniform_grid_spacer(|_| [500.0, 200.0, 50.0]))
                    .custom_y_axes(vec![axis_hints("ms")])
                    .show(ui, |ui| {
                        ui.line(Line::new(rtt).name("RTT").color(main_color));
                        ui.line(Line::new(crtt).name("cRTT").color(weak_color));
                    });
            }

            if self.show_rx_tx {
                plot(history, "rx_tx")
                    .y_grid_spacer(log_grid_spacer(2))
                    .custom_y_axes(vec![axis_hints("bytes/sec")])
                    .y_axis_formatter(fmt_bytes_y_axis)
                    .show(ui, |ui| {
                        ui.line(Line::new(rx).name("Rx").color(self.color_in));
                        ui.line(Line::new(tx).name("Tx").color(self.color_out));
                    });
            }
        });

        ui.horizontal(|ui| {
            ui.label(format!("{:.1?}", now - stats.connected_at));
            ui.separator();

            ui.label(format!("{:.1?} rtt", stats.rtt));
            ui.separator();
            ui.label(format!(
                "{}B rx / {}B tx",
                fmt_bytes(stats.total_bytes_in),
                fmt_bytes(stats.total_bytes_out)
            ));

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.checkbox(&mut self.show_loss, "Loss");
                ui.checkbox(&mut self.show_rx_tx, "Rx/Tx");
                ui.checkbox(&mut self.show_rtt, "RTT");
            });
        });
    }
}

fn plot(history: f64, id_source: impl Hash) -> Plot<'static> {
    Plot::new(id_source)
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
        .y_axis_min_width(48.0)
        .legend(Legend::default().position(Corner::LeftTop))
}

fn axis_hints(label: impl Into<WidgetText>) -> AxisHints<'static> {
    AxisHints::new_y()
        .label(label)
        .placement(Placement::RightTop)
        .min_thickness(48.0)
}

fn fmt_bytes_y_axis(mark: GridMark, _range: &RangeInclusive<f64>) -> String {
    fmt_bytes(mark.value as usize)
}

fn fmt_bytes(bytes: usize) -> String {
    format!(
        "{:.0}",
        SizeFormatter::<usize, BinaryPrefixes, PointSeparated>::new(bytes)
    )
}

fn draw(
    mut egui: EguiContexts,
    mut sessions: Query<(
        Entity,
        Option<&Name>,
        &SessionStats,
        &mut SessionStatsVisualizer,
        &ConnectedAt,
        &Rtt,
        &IoStats,
    )>,
) {
    for (session, name, samples, mut visualizer, connected_at, rtt, &io_stats) in &mut sessions {
        let display_name = display_name(session, name);
        let window_id = format!("aeronet session {session:?}");

        let stats = SessionStatsParam {
            connected_at: **connected_at,
            rtt: rtt.get(),
            io: io_stats,
            sample_rate:
            samples: &samples,
        };

        egui::Window::new(format!("Session {display_name}"))
            .id(egui::Id::new(window_id))
            .show(egui.ctx_mut(), |ui| visualizer.show(ui, &stats));
    }
}
