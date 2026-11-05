//! Dashboard window + tray integration.
//!
//! Closing the window hides it to the tray; the app keeps running until "Quit".

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, FontId, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2,
    ViewportCommand,
};
use tray_icon::menu::MenuEvent;

use crate::state::{HISTORY_WINDOW, Rx, State};
use crate::tray::{Action, Tray};

const ACCENT: Color32 = Color32::from_rgb(0xff, 0x4d, 0x5e);

pub struct Options {
    pub overlay_url: String,
    pub server_error: Option<String>,
    pub start_hidden: bool,
}

pub fn run(rx: Rx, rt: tokio::runtime::Handle, opts: Options) -> eframe::Result {
    let icon = {
        let (w, h, rgba) = crate::tray::heart_rgba(128, [0xff, 0x4d, 0x5e], 0.88);
        egui::IconData {
            rgba,
            width: w,
            height: h,
        }
    };
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Heart Rate")
            .with_app_id("miband-heart-rate")
            .with_inner_size([420.0, 560.0])
            .with_min_inner_size([300.0, 380.0])
            .with_visible(!opts.start_hidden)
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "miband-heart-rate",
        native,
        Box::new(move |cc| {
            // Repaint whenever the BLE state changes (also drives `logic` while hidden).
            let ctx = cc.egui_ctx.clone();
            let mut watch = rx.clone();
            rt.spawn(async move {
                while watch.changed().await.is_ok() {
                    ctx.request_repaint();
                }
            });

            // Tray menu events arrive on another thread (Linux) or outside a frame:
            // queue them and wake the UI.
            let pending: Arc<Mutex<Vec<MenuEvent>>> = Arc::default();
            {
                let pending = pending.clone();
                let ctx = cc.egui_ctx.clone();
                MenuEvent::set_event_handler(Some(move |e| {
                    pending.lock().unwrap().push(e);
                    ctx.request_repaint();
                }));
            }

            let tray = match Tray::new() {
                Ok(t) => Some(t),
                Err(e) => {
                    tracing::warn!("tray unavailable: {e:#}");
                    None
                }
            };
            // Without a tray, hiding the window would make the app unreachable.
            if tray.is_none() && opts.start_hidden {
                cc.egui_ctx
                    .send_viewport_cmd(ViewportCommand::Visible(true));
            }

            Ok(Box::new(App {
                rx,
                opts,
                tray,
                pending,
                quitting: false,
                copied_at: None,
            }))
        }),
    )
}

struct App {
    rx: Rx,
    opts: Options,
    tray: Option<Tray>,
    pending: Arc<Mutex<Vec<MenuEvent>>>,
    quitting: bool,
    copied_at: Option<Instant>,
}

impl App {
    fn handle(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Show => {
                ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(ViewportCommand::Focus);
            }
            Action::CopyUrl => {
                ctx.copy_text(self.opts.overlay_url.clone());
                self.copied_at = Some(Instant::now());
            }
            Action::OpenOverlay => {
                if let Err(e) = open::that_detached(&self.opts.overlay_url) {
                    tracing::warn!("cannot open browser: {e}");
                }
            }
            Action::Quit => {
                self.quitting = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let events: Vec<_> = std::mem::take(&mut *self.pending.lock().unwrap());
        for e in events {
            if let Some(action) = self.tray.as_ref().and_then(|t| t.action_for(&e)) {
                self.handle(ctx, action);
            }
        }

        if let Some(tray) = &mut self.tray {
            let state = self.rx.borrow();
            tray.update(state.bpm(), &state.status.label());
        }

        // Close button → hide to tray (only if there is a tray to come back from).
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting && self.tray.is_some() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let state = self.rx.borrow().clone();
        let frame = egui::Frame::central_panel(ui.style()).inner_margin(28.0);
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            dashboard(ui, &state);
            ui.add_space(8.0);
            footer(ui, self);
        });

        // Keep the "copied" hint and the chart's time axis moving.
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

fn dashboard(ui: &mut egui::Ui, state: &State) {
    let muted = ui.visuals().weak_text_color();
    let strong = ui.visuals().strong_text_color();

    // Status line
    let status = match (&state.status, state.last.and_then(|m| m.contact)) {
        (s, Some(false)) if s.is_connected() => format!("{} · not worn", s.label()),
        (s, _) => s.label(),
    };
    ui.label(RichText::new(status).color(muted).size(14.0));
    ui.add_space(12.0);

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
    let h = (ui.available_height() - 40.0).max(80.0);
    chart(ui, state, h);
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

fn footer(ui: &mut egui::Ui, app: &mut App) {
    let muted = ui.visuals().weak_text_color();
    ui.horizontal(|ui| {
        if let Some(err) = &app.opts.server_error {
            ui.label(
                RichText::new(format!("OBS server not running: {err}"))
                    .color(ACCENT)
                    .size(13.0),
            );
            return;
        }
        ui.label(RichText::new("OBS").color(muted).size(13.0));
        ui.label(RichText::new(&app.opts.overlay_url).size(13.0));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let copied = app
                .copied_at
                .is_some_and(|t| t.elapsed() < Duration::from_secs(2));
            let text = if copied { "copied" } else { "copy" };
            let resp = ui
                .add(egui::Button::new(RichText::new(text).color(ACCENT).size(13.0)).frame(false));
            if resp.clicked() {
                app.handle(ui.ctx(), Action::CopyUrl);
            }
        });
    });
}
