//! Device tab: which band to use, connect / disconnect, and the bands nearby.

use eframe::egui::{self, Align, Layout, RichText};

use super::{App, caption, link, text_button};
use crate::ble::Command;
use crate::settings::SavedDevice;
use crate::state::Status;

pub fn show(ui: &mut egui::Ui, app: &mut App) {
    let state = app.rx.borrow().clone();
    let muted = ui.visuals().weak_text_color();
    let strong = ui.visuals().strong_text_color();
    let saved = state.settings.device.clone();

    // ---- the chosen band ----
    caption(ui, "Your band");
    ui.add_space(2.0);
    match &saved {
        None => {
            ui.label(RichText::new("None chosen yet").size(18.0).color(strong));
            ui.label(
                RichText::new("Pick your band from the list below.")
                    .color(muted)
                    .size(13.0),
            );
        }
        Some(d) => {
            ui.label(RichText::new(&d.name).size(18.0).color(strong));
            ui.label(
                RichText::new(state_line(&state.status))
                    .color(muted)
                    .size(13.0),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                let paused = matches!(state.status, Status::Paused(_));
                if paused {
                    if link(ui, "connect").clicked() {
                        app.send(Command::Connect);
                    }
                } else if link(ui, "disconnect").clicked() {
                    app.send(Command::Disconnect);
                }
                let forget = text_button(ui, RichText::new("forget").color(muted).size(13.0));
                if forget.clicked() {
                    app.send(Command::Forget);
                }
            });
        }
    }

    ui.add_space(28.0);

    // ---- nearby ----
    caption(ui, "Nearby");
    ui.add_space(4.0);

    if state.status.is_connected() || matches!(state.status, Status::Connecting(_)) {
        ui.label(
            RichText::new("Disconnect to see other bands nearby.")
                .color(muted)
                .size(13.0),
        );
        return;
    }
    if matches!(state.status, Status::NoAdapter) {
        ui.label(
            RichText::new("Bluetooth is off or unavailable.")
                .color(muted)
                .size(13.0),
        );
        return;
    }

    if state.nearby.is_empty() {
        ui.label(RichText::new("Searching…").color(strong).size(15.0));
        ui.label(
            RichText::new("Turn on heart rate sharing on the band (Share HR).")
                .color(muted)
                .size(13.0),
        );
        return;
    }

    for d in &state.nearby {
        let is_saved = saved.as_ref().is_some_and(|s| s.id == d.id);
        ui.horizontal(|ui| {
            ui.label(RichText::new(&d.name).size(15.0).color(strong));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                if is_saved {
                    ui.label(RichText::new("yours").color(muted).size(13.0));
                } else if link(ui, "use").clicked() {
                    app.send(Command::Select(SavedDevice {
                        id: d.id.clone(),
                        name: d.name.clone(),
                    }));
                }
                if let Some(rssi) = d.rssi {
                    ui.label(RichText::new(signal(rssi)).color(muted).size(13.0));
                }
            });
        });
        ui.add_space(6.0);
    }

    ui.add_space(14.0);
    ui.label(
        RichText::new(
            "Not sure which one is yours? Hold the band next to the computer: \
             it moves to the top with the strongest signal.",
        )
        .color(muted)
        .size(12.0),
    );
}

fn state_line(status: &Status) -> String {
    match status {
        Status::Connected(_) => "Connected".into(),
        Status::Paused(_) => "Disconnected".into(),
        Status::Searching(_) => "Looking for it… make sure heart rate sharing is on.".into(),
        other => other.label(),
    }
}

/// RSSI as words plus dBm: easier to compare at a glance.
fn signal(rssi: i16) -> String {
    let word = match rssi {
        -60.. => "strong",
        -75..=-61 => "good",
        _ => "weak",
    };
    format!("{word} · {rssi} dBm")
}

#[cfg(test)]
mod tests {
    use super::signal;

    #[test]
    fn signal_words() {
        assert_eq!(signal(-45), "strong · -45 dBm");
        assert_eq!(signal(-60), "strong · -60 dBm");
        assert_eq!(signal(-61), "good · -61 dBm");
        assert_eq!(signal(-90), "weak · -90 dBm");
    }
}
