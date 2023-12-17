use std::{error::Error, mem};

use bevy_egui::egui;

use crate::AppMessage;

/// Kind of log message in a [`LogLine`].
#[derive(Debug, Clone, Copy)]
pub enum LogKind {
    /// Message to/from the server/client.
    Message,
    /// Generic info message.
    Info,
    /// User requested connection.
    Connect,
    /// Transmission error or disconnect event.
    Disconnect,
}

impl LogKind {
    /// Gets the color of this log message.
    pub fn color(&self) -> egui::Color32 {
        match self {
            Self::Message => egui::Color32::GRAY,
            Self::Info => egui::Color32::WHITE,
            Self::Connect => egui::Color32::GREEN,
            Self::Disconnect => egui::Color32::RED,
        }
    }
}

/// Line of output written by a client, exposed to the end user.
#[derive(Debug, Clone)]
pub struct LogLine {
    /// Kind of message that this line represents.
    pub kind: LogKind,
    /// Text in this line.
    pub msg: String,
}

impl LogLine {
    /// Creates a new [`LogLine`].
    pub fn new(kind: LogKind, msg: impl Into<String>) -> Self {
        let msg = msg.into();
        Self { kind, msg }
    }

    /// The user requested to connect to a target.
    pub fn connecting(target: impl AsRef<str>) -> Self {
        let target = target.as_ref();
        Self::new(LogKind::Connect, format!("Connecting to {target}"))
    }

    /// The client connected to a server.
    pub fn connected() -> Self {
        Self::new(LogKind::Info, "Connected")
    }

    /// The client sent a message to the server.
    pub fn send(msg: impl AsRef<str>) -> Self {
        let msg = msg.as_ref();
        Self::new(LogKind::Message, format!("< {msg}"))
    }

    /// The server sent a message to this client.
    pub fn recv(msg: impl AsRef<str>) -> Self {
        let msg = msg.as_ref();
        Self::new(LogKind::Message, format!("> {msg}"))
    }

    /// The client lost connection from its server.
    pub fn disconnected(err: &impl Error) -> Self {
        Self::new(
            LogKind::Disconnect,
            format!("Disconnected: {:#}", aeronet::error::as_pretty(err)),
        )
    }

    /// Creates a rich text value for this log line, which can be added to an
    /// [`egui::Ui`].
    pub fn text(&self) -> egui::RichText {
        egui::RichText::new(&self.msg)
            .color(self.kind.color())
            .font(egui::FontId::monospace(14.0))
    }
}

/// Shows a series of [`LogLine`]s in a vertical [`egui::ScrollArea`].
pub fn log_lines(ui: &mut egui::Ui, log: &[LogLine]) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        for line in log {
            ui.label(line.text());
        }
    });
}

/// Shows a message input buffer using [`egui::TextEdit`].
pub fn msg_buf(ui: &mut egui::Ui, buf: &mut String) -> Option<AppMessage> {
    let resp = ui
        .horizontal(|ui| {
            ui.label("Send");
            ui.add(egui::TextEdit::singleline(buf).hint_text("[enter] to send"))
        })
        .inner;

    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        ui.memory_mut(|m| m.request_focus(resp.id));

        let buf = mem::take(buf).trim().to_string();
        if buf.is_empty() {
            return None;
        } else {
            return Some(AppMessage(buf));
        }
    }

    None
}

pub fn url_buf(ui: &mut egui::Ui, url: &mut String) -> Option<String> {
    let resp = ui
        .horizontal(|ui| {
            ui.label("URL");
            ui.add(
                egui::TextEdit::singleline(url)
                    .hint_text("https://[::1]:25565 | [enter] to connect"),
            )
        })
        .inner;

    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        ui.memory_mut(|m| m.request_focus(resp.id));

        let url = url.trim().to_string();
        if url.is_empty() {
            return None;
        } else {
            return Some(url);
        }
    }

    None
}
