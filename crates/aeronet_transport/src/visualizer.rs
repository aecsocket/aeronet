//! See [`SessionVisualizerPlugin`].

use {
    crate::{
        Transport, TransportConfig,
        sampling::{
            SampleSessionStats, SessionSamplingPlugin, SessionStats, SessionStatsSample,
            SessionStatsSampling,
        },
    },
    aeronet_io::{Session, packet::PacketRtt},
    bevy_app::prelude::*,
    bevy_core::Name,
    bevy_ecs::prelude::*,
    bevy_egui::{
        EguiContexts,
        egui::{self, epaint::Hsva},
    },
    itertools::Itertools,
    ringbuf::traits::Consumer,
    size_format::{BinaryPrefixes, PointSeparated, SizeFormatter},
    std::{hash::Hash, ops::RangeInclusive, time::Duration},
    web_time::Instant,
};

/// Uses [`egui`] to draw [`egui_plot`]s of [`Session`] statistics.
///
/// In [`DrawSessionVisualizer`], any [`Session`] with a [`SessionVisualizer`]
/// and [`SessionStats`] will display an [`egui::Window`] with its session
/// statistics.
///
/// Without this plugin, you can still use [`SessionVisualizer`] manually.
///
/// This automatically adds [`SessionSamplingPlugin`].
#[derive(Debug)]
pub struct SessionVisualizerPlugin;

impl Plugin for SessionVisualizerPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SessionSamplingPlugin>() {
            app.add_plugins(SessionSamplingPlugin);
        }

        app.configure_sets(Update, DrawSessionVisualizer.after(SampleSessionStats))
            .add_systems(Update, draw.in_set(DrawSessionVisualizer));
    }
}

/// System set in which [`SessionVisualizer`]s are drawn via [`egui`].
///
/// This runs after [`SampleSessionStats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub struct DrawSessionVisualizer;

/// State for drawing [`egui_plot`]s of [`SessionStats`].
#[derive(Debug, Clone, Component)]
pub struct SessionVisualizer {
    /// Color which represents incoming data.
    pub rx_color: egui::Color32,
    /// Color which represents outgoing data.
    pub tx_color: egui::Color32,
    /// Whether to draw the RTT graph in [`SessionVisualizer::show_plots`].
    pub show_rtt: bool,
    /// Whether to draw the bytes in/out graph in
    /// [`SessionVisualizer::show_plots`].
    pub show_rx_tx: bool,
    /// Whether to draw the miscellaneous fractional data graph in
    /// [`SessionVisualizer::show_plots`].
    pub show_misc: bool,
}

impl Default for SessionVisualizer {
    fn default() -> Self {
        Self {
            rx_color: Hsva::new(0.6, 0.8, 0.6, 1.0).into(),
            tx_color: Hsva::new(0.04, 0.8, 0.6, 1.0).into(),
            show_rtt: true,
            show_rx_tx: true,
            show_misc: true,
        }
    }
}

/// Sample of data for [`SessionVisualizer::show_rtt`].
#[derive(Debug, Clone, Copy)]
pub struct RttSample {
    /// [`PacketRtt`].
    pub packet_rtt: Duration,
    /// [`Transport::rtt`]'s [`RttEstimator::get`].
    ///
    /// [`RttEstimator::get`]: crate::rtt::RttEstimator::get
    pub msg_rtt: Duration,
}

/// Sample of data for [`SessionVisualizer::show_rx_tx`].
#[derive(Debug, Clone, Copy)]
pub struct RxTxSample {
    /// [`PacketStats::bytes_recv`] difference between this sample and the last.
    pub bytes_recv_delta: usize,
    /// [`PacketStats::bytes_sent`] difference between this sample and the last.
    pub bytes_sent_delta: usize,
}

/// Sample of data for [`SessionVisualizer::show_misc`].
#[derive(Debug, Clone, Copy)]
pub struct MiscSample {
    /// Packet loss as computed in [`SessionStatsSample::loss`].
    pub loss: f64,
    /// [`Transport::memory_used`] over [`TransportConfig::max_memory_usage`].
    pub mem_used: f64,
}

impl SessionVisualizer {
    /// Draws the plot for RTT.
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
                let packet_rtt = sample.packet_rtt.as_secs_f64() * MS_PER_SEC;
                let msg_rtt = sample.msg_rtt.as_secs_f64() * MS_PER_SEC;
                ([x, packet_rtt], [x, msg_rtt])
            })
            .multiunzip::<(Vec<_>, Vec<_>)>();

        let color = ui.visuals().text_color();
        let weak_color = ui.visuals().weak_text_color();
        let history_sec = sampling.history_sec();
        plot(history_sec, "rtt")
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

    /// Draws the plot for amount of incoming and outgoing data per second.
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
                #[expect(clippy::cast_precision_loss, reason = "precision loss is acceptable")]
                let (rx, tx) = (
                    sample.bytes_recv_delta as f64 * sample_rate,
                    sample.bytes_sent_delta as f64 * sample_rate,
                );
                ([x, rx], [x, tx])
            })
            .multiunzip::<(Vec<_>, Vec<_>)>();

        let history_sec = sampling.history_sec();
        plot(history_sec, "rx_tx")
            .y_grid_spacer(egui_plot::log_grid_spacer(2))
            .custom_y_axes(vec![axis_hints("bytes/sec")])
            .y_axis_formatter(fmt_bytes_y_axis)
            .show(ui, |ui| {
                ui.line(egui_plot::Line::new(rx).name("Rx").color(self.rx_color));
                ui.line(egui_plot::Line::new(tx).name("Tx").color(self.tx_color));
            })
    }

    /// Draws the plot for miscellaneous fractional statistics.
    pub fn show_misc(
        &self,
        ui: &mut egui::Ui,
        sampling: SessionStatsSampling,
        samples: impl IntoIterator<Item = MiscSample>,
    ) -> egui_plot::PlotResponse<()> {
        let sample_rate = sampling.rate();

        let (loss, mem_used) = samples
            .into_iter()
            .enumerate()
            .map(|(index, sample)| {
                let x = graph_x(index, sample_rate);
                let loss = sample.loss * 100.0;
                let mem_used = sample.mem_used * 100.0;
                ([x, loss], [x, mem_used])
            })
            .multiunzip::<(Vec<_>, Vec<_>)>();

        let color = ui.visuals().text_color();
        let weak_color = ui.visuals().weak_text_color();
        let history_sec = sampling.history_sec();
        plot(history_sec, "loss")
            .include_y(100.0)
            .y_grid_spacer(egui_plot::uniform_grid_spacer(|_| [100.0, 25.0, 10.0]))
            .custom_y_axes(vec![axis_hints("%")])
            .show(ui, |ui| {
                ui.line(egui_plot::Line::new(loss).name("Pkt Loss").color(color));
                ui.line(
                    egui_plot::Line::new(mem_used)
                        .name("Mem Used")
                        .color(weak_color),
                );
            })
    }

    /// Draws the entire UI.
    pub fn show_plots(
        &self,
        ui: &mut egui::Ui,
        sampling: SessionStatsSampling,
        samples: impl IntoIterator<Item = SessionStatsSample> + Clone,
    ) {
        ui.horizontal(|ui| {
            if self.show_rtt {
                self.show_rtt(
                    ui,
                    sampling,
                    samples.clone().into_iter().map(|sample| RttSample {
                        packet_rtt: sample.packet_rtt.unwrap_or_default(),
                        msg_rtt: sample.msg_rtt,
                    }),
                );
            }

            if self.show_rx_tx {
                self.show_rx_tx(
                    ui,
                    sampling,
                    samples.clone().into_iter().map(|sample| RxTxSample {
                        bytes_recv_delta: sample.packets_delta.bytes_recv.0,
                        bytes_sent_delta: sample.packets_delta.bytes_sent.0,
                    }),
                );
            }

            if self.show_misc {
                self.show_misc(
                    ui,
                    sampling,
                    samples.clone().into_iter().map(|sample| MiscSample {
                        loss: sample.loss,
                        #[expect(
                            clippy::cast_precision_loss,
                            reason = "precision loss is acceptable"
                        )]
                        mem_used: sample.mem_used as f64 / sample.mem_max as f64,
                    }),
                );
            }
        });
    }

    /// Draws a status bar showing current [`Session`] and [`Transport`]
    /// statistics.
    pub fn show_status_bar(
        &mut self,
        ui: &mut egui::Ui,
        now: Instant,
        session: &Session,
        packet_rtt: Option<Duration>,
        transport: &Transport,
        transport_config: &TransportConfig,
    ) {
        let unknown = || "?".to_string();

        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(format!(
                    "{:.1?}",
                    now.saturating_duration_since(session.connected_at())
                ));
                ui.separator();

                ui.label("MTU");
                ui.label(format!("{} ({} min)", session.mtu(), session.min_mtu()));
                ui.separator();

                ui.label("RTT");
                ui.label(format!(
                    "{} pkt / {:.1?} msg",
                    packet_rtt.map_or_else(unknown, |rtt| format!("{rtt:.1?}")),
                    transport.rtt().get(),
                ));
                ui.separator();

                ui.label("MEM");
                ui.label(format!(
                    "{} / {}",
                    fmt_bytes(transport.memory_used()),
                    fmt_bytes(transport_config.max_memory_usage)
                ));
                ui.separator();

                ui.label("TX CAP");
                ui.label(format!(
                    "{} / {}",
                    fmt_bytes(transport.send_bytes_bucket().rem()),
                    fmt_bytes(transport.send_bytes_bucket().cap()),
                ));
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(&mut self.show_misc, "Misc");
                ui.checkbox(&mut self.show_rx_tx, "Rx/Tx");
                ui.checkbox(&mut self.show_rtt, "RTT");
            });
        });
    }
}

fn graph_x(index: usize, sample_rate: f64) -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "precision loss is acceptable")]
    let x = -(index as f64 / sample_rate);
    x
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
    #[expect(
        clippy::cast_sign_loss,
        reason = "input values should never be negative"
    )]
    #[expect(clippy::cast_possible_truncation, reason = "truncation is acceptable")]
    fmt_bytes(mark.value as usize)
}

fn draw(
    mut egui: EguiContexts,
    mut sessions: Query<(
        Entity,
        Option<&Name>,
        &SessionStats,
        &mut SessionVisualizer,
        &Session,
        Option<&PacketRtt>,
        &Transport,
        &TransportConfig,
    )>,
    sampling: Res<SessionStatsSampling>,
) {
    for (entity, name, stats, mut visualizer, session, packet_rtt, transport, transport_config) in
        &mut sessions
    {
        let display_name =
            name.map_or_else(|| entity.to_string(), |name| format!("{name} ({entity})"));

        egui::Window::new(format!("Session: {display_name}")).show(egui.ctx_mut(), |ui| {
            visualizer.show_plots(ui, *sampling, stats.iter().rev().copied());
            visualizer.show_status_bar(
                ui,
                Instant::now(),
                session,
                packet_rtt.map(|x| x.0),
                transport,
                transport_config,
            );
        });
    }
}
