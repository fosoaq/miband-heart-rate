//! Persistent settings: the chosen band and the look of each OBS widget.
//!
//! Stored as JSON in the platform config directory. Missing fields fall back to their
//! defaults, so older files keep loading after new options are added.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::state::Rx;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The band to connect to. `None` until the user picks one.
    pub device: Option<SavedDevice>,
    pub widgets: Widgets,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedDevice {
    /// Platform device id (`bluest::DeviceId` as a string).
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Widgets {
    pub bpm: BpmWidget,
    pub chart: ChartWidget,
    pub stats: StatsWidget,
}

/// The big number with a beating heart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BpmWidget {
    pub size: u32,
    pub color: Rgba,
    pub accent: Rgba,
    pub show_heart: bool,
    pub animate_heart: bool,
    pub show_unit: bool,
    pub show_status: bool,
    pub shadow: bool,
}

impl Default for BpmWidget {
    fn default() -> Self {
        Self {
            size: 160,
            color: Rgba::WHITE,
            accent: Rgba::ACCENT,
            show_heart: true,
            animate_heart: true,
            show_unit: true,
            show_status: true,
            shadow: true,
        }
    }
}

/// Heart rate over the last few minutes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartWidget {
    pub width: u32,
    pub height: u32,
    /// Time span shown, 1–10 minutes.
    pub minutes: u32,
    pub color: Rgba,
    pub line_width: f32,
    pub fill: bool,
    pub show_range: bool,
    pub label_color: Rgba,
    pub shadow: bool,
}

impl Default for ChartWidget {
    fn default() -> Self {
        Self {
            width: 600,
            height: 200,
            minutes: 5,
            color: Rgba::ACCENT,
            line_width: 3.0,
            fill: true,
            show_range: true,
            label_color: Rgba([255, 255, 255, 180]),
            shadow: true,
        }
    }
}

/// Min / avg / max over the history window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatsWidget {
    pub size: u32,
    pub color: Rgba,
    pub label_color: Rgba,
    pub show_min: bool,
    pub show_avg: bool,
    pub show_max: bool,
    pub vertical: bool,
    pub shadow: bool,
}

impl Default for StatsWidget {
    fn default() -> Self {
        Self {
            size: 48,
            color: Rgba::WHITE,
            label_color: Rgba([255, 255, 255, 180]),
            show_min: true,
            show_avg: true,
            show_max: true,
            vertical: false,
            shadow: true,
        }
    }
}

/// sRGB colour with alpha, stored as `#rrggbbaa`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const WHITE: Rgba = Rgba([255, 255, 255, 255]);
    pub const ACCENT: Rgba = Rgba([0xff, 0x4d, 0x5e, 255]);

    pub fn to_hex(self) -> String {
        let [r, g, b, a] = self.0;
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
        match s.len() {
            6 => Some(Rgba([byte(0)?, byte(2)?, byte(4)?, 255])),
            8 => Some(Rgba([byte(0)?, byte(2)?, byte(4)?, byte(6)?])),
            _ => None,
        }
    }
}

impl Serialize for Rgba {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Rgba::from_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("bad colour {s:?}")))
    }
}

pub fn path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("miband-heart-rate")
            .join("settings.json"),
    )
}

pub fn load() -> Settings {
    let Some(path) = path() else {
        return Settings::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("ignoring unreadable {}: {e}", path.display());
            Settings::default()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
        Err(e) => {
            tracing::warn!("cannot read {}: {e}", path.display());
            Settings::default()
        }
    }
}

fn save(settings: &Settings) -> std::io::Result<()> {
    let Some(path) = path() else {
        return Ok(());
    };
    std::fs::create_dir_all(path.parent().expect("has parent"))?;
    // Write then rename, so a crash never leaves a half-written file.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(settings)?)?;
    std::fs::rename(tmp, path)
}

/// Saves settings whenever they change, at most twice a second.
///
/// `pinned_device`: when set (demo mode), this is written as the chosen band instead of
/// whatever is in the state, so playing with the demo never replaces the real band.
pub async fn persist(mut rx: Rx, pinned_device: Option<Option<SavedDevice>>) {
    let effective = |s: &Settings| {
        let mut s = s.clone();
        if let Some(d) = &pinned_device {
            s.device = d.clone();
        }
        s
    };
    let mut saved = effective(&rx.borrow().settings);
    while rx.changed().await.is_ok() {
        if effective(&rx.borrow().settings) == saved {
            continue;
        }
        // Let a burst of changes (e.g. dragging a slider) settle first.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let current = effective(&rx.borrow_and_update().settings);
        if current != saved {
            match save(&current) {
                Ok(()) => saved = current,
                Err(e) => tracing::warn!("cannot save settings: {e}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        assert_eq!(Rgba::from_hex("#ff4d5e"), Some(Rgba::ACCENT));
        assert_eq!(
            Rgba::from_hex("#ffffffb4"),
            Some(Rgba([255, 255, 255, 180]))
        );
        assert_eq!(Rgba::ACCENT.to_hex(), "#ff4d5eff");
        assert_eq!(Rgba::from_hex("ff4d5e"), None);
        assert_eq!(Rgba::from_hex("#ff4d"), None);
        assert_eq!(Rgba::from_hex("#gg4d5e"), None);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let s: Settings = serde_json::from_str(r#"{"widgets":{"chart":{"minutes":2}}}"#).unwrap();
        assert_eq!(s.device, None);
        assert_eq!(s.widgets.chart.minutes, 2);
        assert_eq!(s.widgets.chart.width, ChartWidget::default().width);
        assert_eq!(s.widgets.bpm, BpmWidget::default());
    }

    #[test]
    fn json_roundtrip() {
        let mut s = Settings {
            device: Some(SavedDevice {
                id: "abc".into(),
                name: "Band".into(),
            }),
            ..Settings::default()
        };
        s.widgets.stats.vertical = true;
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}
