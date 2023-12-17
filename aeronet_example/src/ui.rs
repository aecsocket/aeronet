use std::mem;

use bevy_egui::egui;

use crate::{AppMessage, LogLine};

/// Shows a series of [`LogLine`]s in a vertical [`egui::ScrollArea`].
pub fn log_lines(ui: &mut egui::Ui, log: &[LogLine]) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        for line in log {
            ui.label(line.text());
        }
    });
}

// TODO: stupid ass hack because egui WASM adds an "E" after you press enter in
// a TextEdit, so we get rid of it

#[cfg(target_arch = "wasm32")]
fn fix_input(s: &str) -> &str {
    if s.ends_with("E") {
        &s[0..s.len() - 1]
    } else {
        s
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn fix_input(s: &str) -> &str {
    s
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

        let buf = fix_input(mem::take(buf).trim()).to_string();
        return if buf.is_empty() {
            None
        } else {
            Some(AppMessage(buf))
        };
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

        let url = fix_input(url.trim()).to_string();
        return if url.is_empty() { None } else { Some(url) };
    }

    None
}
