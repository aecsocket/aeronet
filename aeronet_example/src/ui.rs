use std::mem;

use bevy_egui::egui;

use crate::LogLine;

/// Shows a series of [`LogLine`]s in a vertical [`egui::ScrollArea`].
pub fn log_lines(ui: &mut egui::Ui, log: &[LogLine]) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        for line in log {
            ui.label(line.text());
        }
    });
}

/// Shows a message input buffer using [`egui::TextEdit`].
pub fn msg_buf(ui: &mut egui::Ui, buf: &mut String) -> Option<String> {
    let resp = ui
        .horizontal(|ui| {
            ui.label("Send");
            ui.add(egui::TextEdit::singleline(buf).hint_text("[enter] to send"))
        })
        .inner;

    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        ui.memory_mut(|m| m.request_focus(resp.id));

        let buf = mem::take(buf).trim().to_string();
        return if buf.is_empty() { None } else { Some(buf) };
    }

    None
}

/// Shows a URL input buffer using [`egui::TextEdit`].
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
        return if url.is_empty() { None } else { Some(url) };
    }

    None
}
