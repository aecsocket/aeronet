//! Provides the [`SessionStatsVisualizer`], allowing you to draw [session]
//! statistics samples using [`egui`].
//!
//! [session]: crate::session

use std::{hash::Hash, ops::RangeInclusive, time::Duration};

use bevy_app::prelude::*;
use bevy_core::Name;
use bevy_ecs::prelude::*;
use bevy_egui::{
    egui::{self, epaint::Hsva, Align, Color32, Layout, WidgetText},
    EguiContexts,
};
use egui_plot::{
    log_grid_spacer, uniform_grid_spacer, AxisHints, Corner, GridMark, Legend, Line, Placement,
    Plot,
};
use itertools::Itertools;
use ringbuf::traits::Consumer;
use size_format::{BinaryPrefixes, PointSeparated, SizeFormatter};
use web_time::Instant;

use crate::{
    io::IoStats,
    session::{ConnectedAt, RttEstimator},
    stats::{SessionStats, SessionStatsSampleSet, SessionStatsSampling},
    util::display_name,
};

/// Handles drawing the [`SessionStatsVisualizer`].
///
/// Each [session] with a [`SessionStatsVisualizer`] component
///
/// [session]: crate::session
#[derive(Debug)]
pub struct SessionStatsVisualizerPlugin;

impl Plugin for SessionStatsVisualizerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, draw.after(SessionStatsSampleSet))
            .observe(setup_visualizer);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Component)]
pub struct SessionStatsVisualizer {
    /// Whether to draw the RTT graph.
    pub show_rtt: bool,
    /// Whether to draw the bytes sent/received per second graph.
    pub show_rx_tx: bool,
    /// Whether to draw the message loss graph.
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
            color_in: Hsva::new(0.6, 0.8, 0.6, 1.0).into(),
            color_out: Hsva::new(0.04, 0.8, 0.6, 1.0).into(),
        }
    }
}

// TODO: can this be a required component? since SessionStats impls FromWorld
fn setup_visualizer(
    trigger: Trigger<OnAdd, SessionStatsVisualizer>,
    with_stats: Query<(), With<SessionStats>>,
    mut commands: Commands,
) {
    let session = trigger.entity();
    if with_stats.get(session).is_err() {
        commands.push(move |world: &mut World| {
            let stats = SessionStats::from_world(world);
            world.entity_mut(session).insert(stats);
        });
    }
}

pub struct VisualizerParams<'a> {
    pub connected_at: Instant,
    pub rtt: Duration,
    pub io_total: IoStats,
    pub sample_interval: Duration,
    pub samples: &'a SessionStats,
}

impl SessionStatsVisualizer {
    pub fn show(&mut self, ui: &mut egui::Ui, params: &VisualizerParams) {
        let now = Instant::now();
        let sample_rate = 1.0 / params.sample_interval.as_secs_f64();
        let history = 15.0;

        let (rtt, crtt, rx, tx, loss): (Vec<_>, Vec<_>, Vec<_>, Vec<_>, Vec<_>) = params
            .samples
            .iter()
            .rev()
            .enumerate()
            .map(|(index, sample)| {
                let x = -(index as f64 / sample_rate);
                (
                    [x, (sample.rtt.as_millis() * 1000) as f64],
                    [x, (sample.conservative_rtt.as_millis() * 1000) as f64],
                    [x, sample.io_delta.bytes_recv.0 as f64 * sample_rate],
                    [x, sample.io_delta.bytes_sent.0 as f64 * sample_rate],
                    [x, sample.msg_loss],
                    // [x, sample.memory_usage as f64],
                    // [x, sample.bytes_used as f64],
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

            if self.show_loss {
                plot(history, "loss")
                    .include_y(1.0)
                    .y_grid_spacer(uniform_grid_spacer(|_| [1.0, 0.25, 0.1]))
                    .custom_y_axes(vec![axis_hints("%")])
                    .y_axis_formatter(fmt_percent)
                    .show(ui, |ui| {
                        ui.line(Line::new(loss).name("Msg Loss").color(main_color));
                    });
            }
        });

        ui.horizontal(|ui| {
            ui.label(format!("{} Hz", sample_rate));
            ui.separator();

            ui.label(format!("{:.1?}", now - params.connected_at));
            ui.separator();

            ui.label(format!("{:.1?} rtt", params.rtt));
            ui.separator();
            ui.label(format!(
                "{}B rx / {}B tx",
                fmt_bytes(params.io_total.bytes_recv.0),
                fmt_bytes(params.io_total.bytes_sent.0)
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
        .height(125.0)
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

fn fmt_percent(mark: GridMark, _range: &RangeInclusive<f64>) -> String {
    format!("{:.0}", mark.value * 100.0)
}

fn draw(
    mut egui: EguiContexts,
    sampling: Res<SessionStatsSampling>,
    mut sessions: Query<(
        Entity,
        Option<&Name>,
        &SessionStats,
        &mut SessionStatsVisualizer,
        &ConnectedAt,
        &RttEstimator,
        &IoStats,
    )>,
) {
    for (session, name, samples, mut visualizer, connected_at, rtt, io_stats) in &mut sessions {
        let display_name = display_name(session, name);
        let window_id = format!("aeronet session {session:?}");

        let params = VisualizerParams {
            connected_at: **connected_at,
            rtt: rtt.get(),
            io_total: *io_stats,
            sample_interval: sampling.interval,
            samples: &samples,
        };

        egui::Window::new(format!("Session {display_name}"))
            .id(egui::Id::new(window_id))
            .show(egui.ctx_mut(), |ui| visualizer.show(ui, &params));
    }
}
