//! Monitor tab: the live number, min / avg / max and a chart.

use std::time::Instant;

use eframe::egui::{
    self, Align, Color32, FontId, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2,
};

use super::{ACCENT, App, caption, url_row};
use crate::state::{HISTORY_WINDOW, State};

pub fn show(ui: &mut egui::Ui, app: &mut App) {
    let state = app.rx.borrow().clone();
    let muted = ui.visuals().weak_text_color();
    let strong = ui.visuals().strong_text_color();

    let status = match (&state.status, state.last.and_then(|m| m.contact)) {
        (s, Some(false)) if s.is_connected() => format!("{} · not worn", s.label()),
        (s, _) => s.label(),
    };
    ui.label(RichText::new(status).color(muted).size(14.0));
    ui.add_space(8.0);

    // Big number
    ui.horizontal(|ui| {
        let text = state.bpm().map_or("--".to_owned(), |b| b.to_string());
        let color = if state.bpm().is_some() { strong } else { muted };
        ui.label(
            RichText::new(text)
                .font(FontId::proportional(96.0))
                .color(color),
        );
        ui.add_space(6.0);
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(18.0);
            ui.label(RichText::new("BPM").color(muted).size(16.0));
        });
    });
    ui.add_space(16.0);

    // Stats
    let stats = state.stats();
    ui.columns(3, |cols| {
        for (col, (label, value)) in cols.iter_mut().zip([
            ("min", stats.map(|s| s.min)),
            ("avg", stats.map(|s| s.avg)),
            ("max", stats.map(|s| s.max)),
        ]) {
            col.label(RichText::new(label).color(muted).size(13.0));
            col.label(
                RichText::new(value.map_or("--".into(), |v| v.to_string()))
                    .color(strong)
                    .size(26.0),
            );
        }
    });
    ui.add_space(20.0);

    // Chart takes the remaining height minus the footer.
    let h = (ui.available_height() - 52.0).max(80.0);
    chart(ui, &state, h);
    ui.add_space(12.0);

    if let Some(err) = &app.opts.server_error {
        ui.label(
            RichText::new(format!("OBS server not running: {err}"))
                .color(ACCENT)
                .size(13.0),
        );
    } else {
        caption(ui, "OBS overlay");
        let url = app.widget_url("bpm");
        url_row(ui, app, url);
    }
}

fn chart(ui: &mut egui::Ui, state: &State, height: f32) {
    let muted = ui.visuals().weak_text_color();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect.expand(2.0));

    let label_w = 28.0;
    let plot = Rect::from_min_max(rect.min, Pos2::new(rect.max.x - label_w, rect.max.y - 18.0));

    let now = Instant::now();
    let pts: Vec<_> = state
        .history
        .iter()
        .map(|s| (now.duration_since(s.at).as_secs_f32(), s.bpm))
        .collect();
    if pts.len() < 2 {
        painter.text(
            plot.center(),
            egui::Align2::CENTER_CENTER,
            "waiting for data",
            FontId::proportional(13.0),
            muted,
        );
        return;
    }

    let (lo, hi) = pts
        .iter()
        .fold((u16::MAX, 0), |(lo, hi), &(_, b)| (lo.min(b), hi.max(b)));
    let pad = ((hi - lo) as f32 * 0.15).max(5.0);
    let (lo_f, hi_f) = (lo as f32 - pad, hi as f32 + pad);
    // Grow the time axis with the data: at least 1 minute, at most the history window.
    let oldest = pts.first().map_or(0.0, |p| p.0);
    let window = oldest.clamp(60.0, HISTORY_WINDOW.as_secs_f32());

    let to_screen = |age: f32, bpm: f32| {
        Pos2::new(
            plot.right() - age / window * plot.width(),
            plot.bottom() - (bpm - lo_f) / (hi_f - lo_f) * plot.height(),
        )
    };

    // Two faint guides at min and max, labelled on the right.
    for v in [lo, hi] {
        let y = to_screen(0.0, v as f32).y;
        painter.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(1.0, muted.gamma_multiply(0.25)),
        );
        painter.text(
            Pos2::new(rect.right(), y),
            egui::Align2::RIGHT_CENTER,
            v.to_string(),
            FontId::proportional(12.0),
            muted,
        );
    }

    let line: Vec<Pos2> = pts
        .iter()
        .map(|&(age, b)| to_screen(age, b as f32))
        .collect();

    // Soft fill under the line, fading to transparent at the bottom.
    let base = plot.bottom();
    let mut mesh = egui::Mesh::default();
    let fill_color = ACCENT.gamma_multiply(0.12);
    for (i, p) in line.iter().enumerate() {
        mesh.colored_vertex(*p, fill_color);
        mesh.colored_vertex(Pos2::new(p.x, base), Color32::TRANSPARENT);
        if i > 0 {
            let k = (i as u32) * 2;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k, k + 1);
        }
    }
    painter.add(Shape::mesh(mesh));
    painter.add(Shape::line(line.clone(), Stroke::new(2.0, ACCENT)));
    // History is oldest-first: the newest reading is the last point.
    if let Some(last) = line.last() {
        painter.circle_filled(*last, 3.5, ACCENT);
    }

    // Time axis: just the two ends.
    let y = rect.bottom() - 6.0;
    let span = format!("{} min", (window / 60.0).round() as u32);
    painter.text(
        Pos2::new(plot.left(), y),
        egui::Align2::LEFT_CENTER,
        span,
        FontId::proportional(12.0),
        muted,
    );
    painter.text(
        Pos2::new(plot.right(), y),
        egui::Align2::RIGHT_CENTER,
        "now",
        FontId::proportional(12.0),
        muted,
    );
}
