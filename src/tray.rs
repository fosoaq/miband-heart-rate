//! System tray / menu bar icon.
//!
//! macOS: monochrome template icon + live BPM as the menu bar title.
//! Windows / Linux: red heart icon, live BPM in the tooltip.

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Show,
    CopyUrl,
    OpenOverlay,
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    status_item: MenuItem,
    ids: [(MenuId, Action); 4],
    last_text: Option<String>,
}

impl Tray {
    pub fn new() -> anyhow::Result<Self> {
        let status_item = MenuItem::new("Starting…", false, None);
        let show = MenuItem::new("Show window", true, None);
        let copy = MenuItem::new("Copy OBS overlay URL", true, None);
        let open = MenuItem::new("Open overlay in browser", true, None);
        let quit = MenuItem::new("Quit", true, None);

        let menu = Menu::new();
        menu.append_items(&[
            &status_item,
            &PredefinedMenuItem::separator(),
            &show,
            &copy,
            &open,
            &PredefinedMenuItem::separator(),
            &quit,
        ])?;

        let builder = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("Heart Rate");
        // macOS draws a monochrome template icon tinted to match the menu bar.
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(heart_icon(true));
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(heart_icon(false));

        Ok(Self {
            icon: builder.build()?,
            status_item,
            ids: [
                (show.id().clone(), Action::Show),
                (copy.id().clone(), Action::CopyUrl),
                (open.id().clone(), Action::OpenOverlay),
                (quit.id().clone(), Action::Quit),
            ],
            last_text: None,
        })
    }

    pub fn action_for(&self, event: &MenuEvent) -> Option<Action> {
        self.ids.iter().find(|(id, _)| *id == event.id).map(|(_, a)| *a)
    }

    /// Updates the tray with the current reading. Cheap to call every frame.
    pub fn update(&mut self, bpm: Option<u16>, status: &str) {
        let text = match bpm {
            Some(b) => format!("{b}"),
            None => "--".into(),
        };
        let key = format!("{text}|{status}");
        if self.last_text.as_deref() == Some(&key) {
            return;
        }
        self.last_text = Some(key);

        self.status_item.set_text(status);
        let tooltip = match bpm {
            Some(b) => format!("Heart rate: {b} BPM"),
            None => format!("Heart rate: {status}"),
        };
        let _ = self.icon.set_tooltip(Some(tooltip));
        // Shown next to the icon in the macOS menu bar (and as a label on some Linux trays).
        self.icon.set_title(Some(text));
    }
}

/// Renders a heart shape into an RGBA icon.
pub fn heart_icon(template: bool) -> Icon {
    let (w, h, rgba) = heart_rgba(64, if template { [0, 0, 0] } else { [0xff, 0x4d, 0x5e] });
    Icon::from_rgba(rgba, w, h).expect("valid icon")
}

/// Heart via the implicit curve (x² + y² − 1)³ − x²y³ ≤ 0, 4×4 supersampled.
pub fn heart_rgba(size: u32, rgb: [u8; 3]) -> (u32, u32, Vec<u8>) {
    const SS: u32 = 4;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let mut hits = 0;
            for sy in 0..SS {
                for sx in 0..SS {
                    let fx = (px as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let fy = (py as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    let x = (fx - 0.5) * 2.6;
                    let y = (0.47 - fy) * 2.6;
                    let a = x * x + y * y - 1.0;
                    if a * a * a - x * x * y * y * y <= 0.0 {
                        hits += 1;
                    }
                }
            }
            let alpha = (hits * 255 / (SS * SS)) as u8;
            out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
        }
    }
    (size, size, out)
}
