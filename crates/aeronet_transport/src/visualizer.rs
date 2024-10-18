use std::{borrow::Borrow, hash::Hash, ops::RangeInclusive, time::Duration};

use bevy_app::prelude::*;
use bevy_core::Name;
use bevy_ecs::prelude::*;
use bevy_egui::{
    egui::{self, epaint::Hsva},
    EguiContexts,
};
use itertools::Itertools;
use ringbuf::traits::Consumer;
use size_format::{BinaryPrefixes, PointSeparated, SizeFormatter};

use crate::stats::{SessionStats, SessionStatsPlugin, SessionStatsSample, SessionStatsSampling};

#[derive(Debug)]
pub struct SessionVisualizerPlugin;

impl Plugin for SessionVisualizerPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SessionStatsPlugin>() {
            app.add_plugins(SessionStatsPlugin::default());
        }

        app.configure_sets(Update, DrawSessionVisualizer)
            .add_systems(Update, draw.in_set(DrawSessionVisualizer));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub struct DrawSessionVisualizer;

#[derive(Debug, Clone, Component)]
pub struct SessionVisualizer {
    pub rx_color: egui::Color32,
    pub tx_color: egui::Color32,
}

impl Default for SessionVisualizer {
    fn default() -> Self {
        Self {
            rx_color: Hsva::new(0.6, 0.8, 0.6, 1.0).into(),
            tx_color: Hsva::new(0.04, 0.8, 0.6, 1.0).into(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RttSample {
    pub packet_rtt: Duration,
    pub msg_rtt: Duration,
}

#[derive(Debug, Clone, Copy)]
pub struct RxTxSample {
    pub bytes_recv_delta: usize,
    pub bytes_sent_delta: usize,
}

impl SessionVisualizer {
    pub fn show_rtt(
        &self,
        ui: &mut egui::Ui,
        sampling: SessionStatsSampling,
        samples: impl IntoIterator<Item = RttSample>,
    ) -> egui_plot::PlotResponse<()> {
        const MS_PER_SEC: f64 = 1000.0;

        let sample_rate = sampling.rate();

        let (packet_rtt, msg_rtt) = samples
            .into_iter()
            .enumerate()
            .map(|(index, sample)| {
                let x = graph_x(index, sample_rate);
                let sample = sample.borrow();
                (
                    [x, sample.packet_rtt.as_secs_f64() * MS_PER_SEC],
                    [x, sample.msg_rtt.as_secs_f64() * MS_PER_SEC],
                )
            })
            .multiunzip::<(Vec<_>, Vec<_>)>();

        let color = ui.visuals().text_color();
        let weak_color = ui.visuals().weak_text_color();
        plot(sampling.history_sec(), "rtt")
            .y_grid_spacer(egui_plot::uniform_grid_spacer(|_| [500.0, 200.0, 50.0]))
            .custom_y_axes(vec![axis_hints("ms")])
            .show(ui, |ui| {
                ui.line(egui_plot::Line::new(msg_rtt).name("Msg RTT").color(color));
                ui.line(
                    egui_plot::Line::new(packet_rtt)
                        .name("Pkt RTT")
                        .color(weak_color),
                );
            })
    }

    pub fn show_rx_tx(
        &self,
        ui: &mut egui::Ui,
        sampling: SessionStatsSampling,
        samples: impl IntoIterator<Item = RxTxSample>,
    ) -> egui_plot::PlotResponse<()> {
        let sample_rate = sampling.rate();

        let (rx, tx) = samples
            .into_iter()
            .enumerate()
            .map(|(index, sample)| {
                let x = graph_x(index, sample_rate);
                let sample = sample.borrow();
                (
                    [x, sample.bytes_recv_delta as f64 * sample_rate],
                    [x, sample.bytes_sent_delta as f64 * sample_rate],
                )
            })
            .multiunzip::<(Vec<_>, Vec<_>)>();

        plot(sampling.history_sec(), "rx_tx")
            .y_grid_spacer(egui_plot::log_grid_spacer(2))
            .custom_y_axes(vec![axis_hints("bytes/sec")])
            .y_axis_formatter(fmt_bytes_y_axis)
            .show(ui, |ui| {
                ui.line(egui_plot::Line::new(rx).name("Rx").color(self.rx_color));
                ui.line(egui_plot::Line::new(tx).name("Tx").color(self.tx_color));
            })
    }

    pub fn show(
        &self,
        ui: &mut egui::Ui,
        sampling: SessionStatsSampling,
        samples: impl IntoIterator<Item = SessionStatsSample> + Clone,
    ) {
        ui.horizontal(|ui| {
            self.show_rtt(
                ui,
                sampling,
                samples.clone().into_iter().map(|sample| RttSample {
                    packet_rtt: sample.packet_rtt.unwrap_or_default(),
                    msg_rtt: Duration::ZERO,
                }),
            );

            self.show_rx_tx(
                ui,
                sampling,
                samples.clone().into_iter().map(|sample| RxTxSample {
                    bytes_recv_delta: sample.packets_delta.bytes_recv.0,
                    bytes_sent_delta: sample.packets_delta.bytes_sent.0,
                }),
            );
        });
    }
}

fn graph_x(index: usize, sample_rate: f64) -> f64 {
    -(index as f64 / sample_rate)
}

fn plot(history_sec: f64, id_salt: impl Hash) -> egui_plot::Plot<'static> {
    egui_plot::Plot::new(id_salt)
        .height(150.0)
        .view_aspect(2.5)
        .allow_drag([true, false])
        .allow_zoom([true, false])
        .allow_scroll([true, false])
        .allow_boxed_zoom(false)
        .set_margin_fraction([0.0, 0.05].into())
        .include_x(-history_sec)
        .include_x(0.0)
        .include_y(0.0)
        .x_axis_label("sec")
        .x_grid_spacer(egui_plot::uniform_grid_spacer(|_| [10.0, 5.0, 1.0]))
        .y_axis_min_width(48.0)
        .legend(egui_plot::Legend::default().position(egui_plot::Corner::LeftTop))
}

fn axis_hints(label: impl Into<egui::WidgetText>) -> egui_plot::AxisHints<'static> {
    egui_plot::AxisHints::new_y()
        .label(label)
        .placement(egui_plot::Placement::RightTop)
        .min_thickness(48.0)
}

fn fmt_bytes(n: usize) -> String {
    format!(
        "{:.0}",
        SizeFormatter::<_, BinaryPrefixes, PointSeparated>::new(n)
    )
}

fn fmt_bytes_y_axis(mark: egui_plot::GridMark, _range: &RangeInclusive<f64>) -> String {
    fmt_bytes(mark.value as usize)
}

fn draw(
    mut egui: EguiContexts,
    sessions: Query<(Entity, Option<&Name>, &SessionVisualizer, &SessionStats)>,
    sampling: Res<SessionStatsSampling>,
) {
    for (entity, name, visualizer, stats) in &sessions {
        let display_name =
            name.map_or_else(|| entity.to_string(), |name| format!("{name} ({entity})"));

        egui::Window::new(format!("Session: {display_name}")).show(egui.ctx_mut(), |ui| {
            visualizer.show(ui, *sampling, stats.iter().rev().copied());
        });
    }
}
