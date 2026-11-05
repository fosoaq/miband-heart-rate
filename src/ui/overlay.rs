//! Overlay tab: the widget list on the left, the selected widget's settings on the right. Every change is pushed to the
//! widgets immediately (and saved), so OBS updates while you drag a slider.

use eframe::egui::{self, Color32, RichText, Slider, color_picker::Alpha};

use super::{App, caption, text_button, url_row};
use crate::settings::{BpmWidget, ChartWidget, Rgba, StatsWidget, Widgets};

/// Which widget's settings are shown on the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Widget {
    #[default]
    Bpm,
    Chart,
    Stats,
}

impl Widget {
    const ALL: [Widget; 3] = [Widget::Bpm, Widget::Chart, Widget::Stats];

    fn title(self) -> &'static str {
        match self {
            Widget::Bpm => "Heart rate",
            Widget::Chart => "Chart",
            Widget::Stats => "Min / avg / max",
        }
    }

    fn kind(self) -> &'static str {
        match self {
            Widget::Bpm => "bpm",
            Widget::Chart => "chart",
            Widget::Stats => "stats",
        }
    }
}

/// Width of the widget list on the left.
const LIST_WIDTH: f32 = 150.0;

pub fn show(ui: &mut egui::Ui, app: &mut App) {
    let original = app.rx.borrow().settings.widgets.clone();
    let mut w = original.clone();
    let muted = ui.visuals().weak_text_color();
    let strong = ui.visuals().strong_text_color();

    if let Some(err) = &app.opts.server_error {
        ui.label(RichText::new(format!("OBS server not running: {err}")).color(super::ACCENT));
        ui.add_space(12.0);
    }

    ui.horizontal_top(|ui| {
        // ---- left: widget list ----
        ui.vertical(|ui| {
            ui.set_width(LIST_WIDTH);
            for widget in Widget::ALL {
                let color = if app.overlay_widget == widget {
                    strong
                } else {
                    muted
                };
                if text_button(ui, RichText::new(widget.title()).size(15.0).color(color)).clicked()
                {
                    app.overlay_widget = widget;
                }
                ui.add_space(4.0);
            }
            ui.add_space(18.0);
            ui.label(
                RichText::new(
                    "Add each widget as its own Browser source in OBS. Changes show up right away.",
                )
                .color(muted)
                .size(12.0),
            );
            ui.add_space(14.0);
            if text_button(ui, RichText::new("reset all").color(muted).size(13.0)).clicked() {
                w = Widgets::default();
            }
        });

        ui.add_space(28.0);

        // ---- right: settings of the selected widget ----
        ui.vertical(|ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().slider_width = 220.0;
                    let widget = app.overlay_widget;
                    ui.label(RichText::new(widget.title()).size(17.0).color(strong));
                    let url = app.widget_url(widget.kind());
                    url_row(ui, app, url);
                    ui.add_space(12.0);
                    egui::Grid::new(widget.kind())
                        .num_columns(2)
                        .spacing([18.0, 12.0])
                        .show(ui, |ui| match widget {
                            Widget::Bpm => bpm(ui, &mut w.bpm),
                            Widget::Chart => chart(ui, &mut w.chart),
                            Widget::Stats => stats(ui, &mut w.stats),
                        });
                });
        });
    });

    if w != original {
        app.tx.send_modify(|s| s.settings.widgets = w);
    }
}

fn bpm(ui: &mut egui::Ui, w: &mut BpmWidget) {
    row(ui, "Size", |ui| {
        ui.add(Slider::new(&mut w.size, 40..=400).suffix(" px"))
    });
    row(ui, "Number", |ui| color(ui, &mut w.color));
    row(ui, "Heart", |ui| color(ui, &mut w.accent));
    row(ui, "Show", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut w.show_heart, "heart");
            ui.add_enabled(
                w.show_heart,
                egui::Checkbox::new(&mut w.animate_heart, "beat with the pulse"),
            );
            ui.checkbox(&mut w.show_unit, "“BPM” label");
            ui.checkbox(&mut w.show_status, "status while disconnected");
            ui.checkbox(&mut w.shadow, "shadow");
        })
        .response
    });
    reset_row(ui, w, BpmWidget::default());
}

fn chart(ui: &mut egui::Ui, w: &mut ChartWidget) {
    row(ui, "Width", |ui| {
        ui.add(Slider::new(&mut w.width, 200..=1600).suffix(" px"))
    });
    row(ui, "Height", |ui| {
        ui.add(Slider::new(&mut w.height, 60..=600).suffix(" px"))
    });
    row(ui, "Time span", |ui| {
        ui.add(Slider::new(&mut w.minutes, 1..=10).suffix(" min"))
    });
    row(ui, "Line", |ui| color(ui, &mut w.color));
    row(ui, "Line width", |ui| {
        ui.add(Slider::new(&mut w.line_width, 1.0..=10.0).step_by(0.5))
    });
    row(ui, "Labels", |ui| color(ui, &mut w.label_color));
    row(ui, "Show", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut w.fill, "fill under the line");
            ui.checkbox(&mut w.show_range, "min / max labels");
            ui.checkbox(&mut w.shadow, "shadow");
        })
        .response
    });
    row(ui, "", |ui| {
        ui.label(
            RichText::new(format!(
                "Set the OBS source to at least {}×{}.",
                w.width, w.height
            ))
            .color(ui.visuals().weak_text_color())
            .size(12.0),
        )
    });
    reset_row(ui, w, ChartWidget::default());
}

fn stats(ui: &mut egui::Ui, w: &mut StatsWidget) {
    row(ui, "Size", |ui| {
        ui.add(Slider::new(&mut w.size, 16..=160).suffix(" px"))
    });
    row(ui, "Numbers", |ui| color(ui, &mut w.color));
    row(ui, "Labels", |ui| color(ui, &mut w.label_color));
    row(ui, "Show", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut w.show_min, "min");
            ui.checkbox(&mut w.show_avg, "avg");
            ui.checkbox(&mut w.show_max, "max");
            ui.checkbox(&mut w.shadow, "shadow");
        })
        .response
    });
    row(ui, "Layout", |ui| {
        ui.horizontal(|ui| {
            ui.radio_value(&mut w.vertical, false, "row");
            ui.radio_value(&mut w.vertical, true, "column");
        })
        .response
    });
    reset_row(ui, w, StatsWidget::default());
}

fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui) -> egui::Response) {
    caption(ui, label);
    add(ui);
    ui.end_row();
}

fn reset_row<T: PartialEq>(ui: &mut egui::Ui, value: &mut T, default: T) {
    ui.label("");
    let muted = ui.visuals().weak_text_color();
    let enabled = *value != default;
    if ui
        .add_enabled(
            enabled,
            egui::Button::new(RichText::new("reset").color(muted).size(13.0)).frame(false),
        )
        .clicked()
    {
        *value = default;
    }
    ui.end_row();
}

/// Colour button editing an [`Rgba`] (with alpha, for semi-transparent overlays).
fn color(ui: &mut egui::Ui, c: &mut Rgba) -> egui::Response {
    let [r, g, b, a] = c.0;
    let mut c32 = Color32::from_rgba_unmultiplied(r, g, b, a);
    let resp = egui::color_picker::color_edit_button_srgba(ui, &mut c32, Alpha::OnlyBlend);
    if resp.changed() {
        *c = Rgba(c32.to_srgba_unmultiplied());
    }
    resp
}
