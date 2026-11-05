//! The window (Monitor / Device / Overlay tabs) and the tray.
//!
//! Closing the window hides it to the tray; the app keeps running until "Quit".

mod device;
mod monitor;
mod overlay;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText, ViewportCommand};
use tokio::sync::mpsc::UnboundedSender;
use tray_icon::menu::MenuEvent;

use crate::ble::Command;
use crate::state::{Rx, Tx};
use crate::tray::{Action, Tray};

const ACCENT: Color32 = Color32::from_rgb(0xff, 0x4d, 0x5e);

pub struct Options {
    /// e.g. `http://127.0.0.1:3030`
    pub base_url: String,
    pub server_error: Option<String>,
    pub start_hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Monitor,
    Device,
    Overlay,
}

pub fn run(
    tx: Tx,
    cmds: UnboundedSender<Command>,
    rt: tokio::runtime::Handle,
    opts: Options,
) -> eframe::Result {
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
            .with_inner_size([720.0, 540.0])
            .with_min_inner_size([560.0, 440.0])
            .with_visible(!opts.start_hidden)
            .with_icon(icon),
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native(
        "miband-heart-rate",
        native,
        Box::new(move |cc| {
            let rx = tx.subscribe();

            // Repaint whenever the state changes (also drives `logic` while hidden).
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

            // First run: nothing to show until a band is chosen.
            let tab = if rx.borrow().settings.device.is_none() {
                Tab::Device
            } else {
                Tab::Monitor
            };

            Ok(Box::new(App {
                tx,
                rx,
                cmds,
                opts,
                tray,
                pending,
                quitting: false,
                tab,
                overlay_widget: overlay::Widget::default(),
                copied: None,
            }))
        }),
    )
}

pub(crate) struct App {
    tx: Tx,
    rx: Rx,
    cmds: UnboundedSender<Command>,
    opts: Options,
    tray: Option<Tray>,
    pending: Arc<Mutex<Vec<MenuEvent>>>,
    quitting: bool,
    tab: Tab,
    overlay_widget: overlay::Widget,
    /// Which URL was copied last, and when (for the "copied" hint).
    copied: Option<(String, Instant)>,
}

impl App {
    fn widget_url(&self, kind: &str) -> String {
        format!("{}/widget/{kind}", self.opts.base_url)
    }

    fn send(&self, cmd: Command) {
        if self.cmds.send(cmd).is_err() {
            tracing::warn!("bluetooth task is gone");
        }
    }

    fn copy(&mut self, ctx: &egui::Context, url: String) {
        ctx.copy_text(url.clone());
        self.copied = Some((url, Instant::now()));
    }

    fn just_copied(&self, url: &str) -> bool {
        self.copied
            .as_ref()
            .is_some_and(|(u, t)| u == url && t.elapsed() < Duration::from_secs(2))
    }

    fn handle(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Show => {
                ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(ViewportCommand::Focus);
            }
            Action::CopyUrl => {
                let url = self.widget_url("bpm");
                self.copy(ctx, url);
            }
            Action::OpenOverlay => {
                if let Err(e) = open::that_detached(self.widget_url("bpm")) {
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
        let frame =
            egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(28, 22));
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            tabs(ui, &mut self.tab);
            ui.add_space(18.0);
            match self.tab {
                Tab::Monitor => monitor::show(ui, self),
                Tab::Device => device::show(ui, self),
                Tab::Overlay => overlay::show(ui, self),
            }
        });

        // Keep the "copied" hints and the chart's time axis moving.
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

/// Plain text tabs: the current one is strong, the others muted. No boxes.
fn tabs(ui: &mut egui::Ui, tab: &mut Tab) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 20.0;
        for (t, label) in [
            (Tab::Monitor, "Monitor"),
            (Tab::Device, "Device"),
            (Tab::Overlay, "Overlay"),
        ] {
            let color = if *tab == t {
                ui.visuals().strong_text_color()
            } else {
                ui.visuals().weak_text_color()
            };
            if text_button(ui, RichText::new(label).size(15.0).color(color)).clicked() {
                *tab = t;
            }
        }
    });
}

/// A frameless, text-only button.
fn text_button(ui: &mut egui::Ui, text: RichText) -> egui::Response {
    ui.add(egui::Button::new(text).frame(false))
}

/// Accent-coloured text action, e.g. "copy".
fn link(ui: &mut egui::Ui, text: &str) -> egui::Response {
    text_button(ui, RichText::new(text).color(ACCENT).size(13.0))
}

/// Small muted caption above a section.
fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .color(ui.visuals().weak_text_color())
            .size(13.0),
    );
}

/// A URL with "copy" (and "open") actions.
fn url_row(ui: &mut egui::Ui, app: &mut App, url: String) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(&url).size(13.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if link(ui, "open").clicked()
                && let Err(e) = open::that_detached(&url)
            {
                tracing::warn!("cannot open browser: {e}");
            }
            let text = if app.just_copied(&url) {
                "copied"
            } else {
                "copy"
            };
            if link(ui, text).clicked() {
                app.copy(ui.ctx(), url.clone());
            }
        });
    });
}
