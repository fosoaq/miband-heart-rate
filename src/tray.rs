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

        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Heart Rate");
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
        self.ids
            .iter()
            .find(|(id, _)| *id == event.id)
            .map(|(_, a)| *a)
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
///
/// The macOS menu bar scales the image to its full 22 pt height, while system glyphs
/// are ~14–16 pt, so the template variant leaves transparent padding around the heart.
pub fn heart_icon(template: bool) -> Icon {
    let (w, h, rgba) = if template {
        heart_rgba(64, [0, 0, 0], 0.66)
    } else {
        heart_rgba(64, [0xff, 0x4d, 0x5e], 0.88)
    };
    Icon::from_rgba(rgba, w, h).expect("valid icon")
}

/// Heart via the implicit curve (x² + y² − 1)³ − x²y³ ≤ 0, 4×4 supersampled.
/// `fill` is the share of the canvas width the heart spans (0..1), centred.
pub fn heart_rgba(size: u32, rgb: [u8; 3], fill: f32) -> (u32, u32, Vec<u8>) {
    const SS: u32 = 4;
    // The curve spans x ∈ [-1.14, 1.14] and y ∈ [-1, 1.23]; scale and centre it.
    const HALF_WIDTH: f32 = 1.14;
    const Y_CENTER: f32 = 0.115;
    let k = 2.0 * HALF_WIDTH / fill;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let mut hits = 0;
            for sy in 0..SS {
                for sx in 0..SS {
                    let fx = (px as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let fy = (py as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    let x = (fx - 0.5) * k;
                    let y = Y_CENTER + (0.5 - fy) * k;
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

#[cfg(test)]
mod tests {
    use super::heart_rgba;

    /// Bounding box (x0, y0, x1, y1) of pixels that are at least half opaque.
    fn bbox(size: u32, rgba: &[u8]) -> (u32, u32, u32, u32) {
        let (mut x0, mut y0, mut x1, mut y1) = (size, size, 0, 0);
        for y in 0..size {
            for x in 0..size {
                if rgba[((y * size + x) * 4 + 3) as usize] >= 128 {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                }
            }
        }
        (x0, y0, x1, y1)
    }

    #[test]
    fn heart_fills_requested_share_and_is_centred() {
        for fill in [0.66, 0.88] {
            let (size, _, rgba) = heart_rgba(64, [0, 0, 0], fill);
            let (x0, y0, x1, y1) = bbox(size, &rgba);
            let width = (x1 - x0 + 1) as f32 / size as f32;
            assert!((width - fill).abs() < 0.05, "fill {fill}: width {width}");
            // Centred within a pixel or two on both axes.
            assert!(
                (x0 as i32 - (size - 1 - x1) as i32).abs() <= 1,
                "fill {fill}: x {x0}..{x1}"
            );
            assert!(
                (y0 as i32 - (size - 1 - y1) as i32).abs() <= 2,
                "fill {fill}: y {y0}..{y1}"
            );
        }
    }
}
