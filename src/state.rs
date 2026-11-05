//! Shared application state, published through a `tokio::sync::watch` channel.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::hr::Measurement;
use crate::settings::Settings;

/// How much history to keep for the chart and min/avg/max.
pub const HISTORY_WINDOW: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum Status {
    Starting,
    NoAdapter,
    /// No band chosen yet: scanning so the user can pick one.
    ChooseDevice,
    /// Looking for the chosen band.
    Searching(String),
    Connecting(String),
    Connected(String),
    /// The user disconnected; the chosen band is kept.
    Paused(String),
    Error(String),
}

impl Status {
    pub fn label(&self) -> String {
        match self {
            Status::Starting => "Starting…".into(),
            Status::NoAdapter => "Bluetooth unavailable".into(),
            Status::ChooseDevice => "Choose your band".into(),
            Status::Searching(name) => format!("Looking for {name}…"),
            Status::Connecting(name) => format!("Connecting to {name}…"),
            Status::Connected(name) => name.clone(),
            Status::Paused(name) => format!("Disconnected from {name}"),
            Status::Error(msg) => msg.clone(),
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, Status::Connected(_))
    }
}

#[derive(Debug, Clone)]
pub struct Sample {
    pub at: Instant,
    pub bpm: u16,
}

/// A band seen while scanning.
#[derive(Debug, Clone, PartialEq)]
pub struct NearbyDevice {
    pub id: String,
    pub name: String,
    pub rssi: Option<i16>,
    pub last_seen: Instant,
}

#[derive(Debug, Clone)]
pub struct State {
    pub status: Status,
    pub last: Option<Measurement>,
    pub history: VecDeque<Sample>,
    /// Bands advertising heart rate nearby, strongest signal first.
    pub nearby: Vec<NearbyDevice>,
    pub settings: Settings,
}

impl State {
    pub fn new(settings: Settings) -> Self {
        Self {
            status: Status::Starting,
            last: None,
            history: VecDeque::new(),
            nearby: Vec::new(),
            settings,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Stats {
    pub min: u16,
    pub avg: u16,
    pub max: u16,
}

/// Nearby devices not heard from for this long are dropped from the list.
const NEARBY_TTL: Duration = Duration::from_secs(10);

impl State {
    pub fn set_status(&mut self, status: Status) {
        if !status.is_connected() {
            self.last = None;
        }
        if self.status != status {
            self.status = status;
        }
    }

    pub fn push(&mut self, m: Measurement) {
        let now = Instant::now();
        self.last = Some(m);
        self.history.push_back(Sample {
            at: now,
            bpm: m.bpm,
        });
        while self
            .history
            .front()
            .is_some_and(|s| now.duration_since(s.at) > HISTORY_WINDOW)
        {
            self.history.pop_front();
        }
    }

    /// Records an advertisement. Returns `true` if the visible list changed
    /// (new device, new name or a noticeably different signal).
    pub fn saw(&mut self, id: String, name: String, rssi: Option<i16>) -> bool {
        let now = Instant::now();
        if let Some(d) = self.nearby.iter_mut().find(|d| d.id == id) {
            let changed = d.name != name
                || match (d.rssi, rssi) {
                    (Some(a), Some(b)) => (a - b).abs() >= 3,
                    (a, b) => a != b,
                };
            d.last_seen = now;
            if changed {
                d.name = name;
                d.rssi = rssi;
                self.sort_nearby();
            }
            changed
        } else {
            self.nearby.push(NearbyDevice {
                id,
                name,
                rssi,
                last_seen: now,
            });
            self.sort_nearby();
            true
        }
    }

    /// Drops devices that went quiet. Returns `true` if any were removed.
    pub fn prune_nearby(&mut self) -> bool {
        let before = self.nearby.len();
        self.nearby.retain(|d| d.last_seen.elapsed() < NEARBY_TTL);
        self.nearby.len() != before
    }

    fn sort_nearby(&mut self) {
        self.nearby
            .sort_by_key(|d| std::cmp::Reverse(d.rssi.unwrap_or(i16::MIN)));
    }

    /// Current BPM, only while connected.
    pub fn bpm(&self) -> Option<u16> {
        self.status.is_connected().then_some(self.last?.bpm)
    }

    pub fn stats(&self) -> Option<Stats> {
        let n = self.history.len() as u32;
        if n == 0 {
            return None;
        }
        let (mut min, mut max, mut sum) = (u16::MAX, 0, 0u32);
        for s in &self.history {
            min = min.min(s.bpm);
            max = max.max(s.bpm);
            sum += u32::from(s.bpm);
        }
        Some(Stats {
            min,
            avg: ((sum + n / 2) / n) as u16,
            max,
        })
    }

    /// The public JSON document. `history` adds `[seconds_ago, bpm]` pairs, oldest first.
    pub fn to_json(&self, history: bool) -> serde_json::Value {
        let mut v = serde_json::json!({
            "bpm": self.bpm(),
            "connected": self.status.is_connected(),
            "contact": self.last.and_then(|m| m.contact),
            "status": self.status,
            "status_text": self.status.label(),
            "stats": self.stats(),
            "widgets": self.settings.widgets,
        });
        if history {
            let now = Instant::now();
            let points: Vec<_> = self
                .history
                .iter()
                .map(|s| {
                    let age = now.duration_since(s.at).as_secs_f32();
                    serde_json::json!([(age * 10.0).round() / 10.0, s.bpm])
                })
                .collect();
            v["history"] = points.into();
        }
        v
    }
}

pub type Tx = tokio::sync::watch::Sender<State>;
pub type Rx = tokio::sync::watch::Receiver<State>;

pub fn channel(settings: Settings) -> (Tx, Rx) {
    tokio::sync::watch::channel(State::new(settings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(bpm: u16) -> Measurement {
        Measurement {
            bpm,
            contact: Some(true),
        }
    }

    #[test]
    fn stats_and_bpm_visibility() {
        let mut s = State::new(Settings::default());
        assert_eq!(s.stats(), None);
        s.set_status(Status::Connected("Band".into()));
        for b in [60, 70, 81] {
            s.push(m(b));
        }
        assert_eq!(s.bpm(), Some(81));
        assert_eq!(
            s.stats(),
            Some(Stats {
                min: 60,
                avg: 70,
                max: 81
            })
        );

        s.set_status(Status::Searching("Band".into()));
        assert_eq!(s.bpm(), None);
        // history survives a reconnect
        assert_eq!(s.stats().unwrap().max, 81);
    }

    #[test]
    fn nearby_sorted_by_signal_and_deduplicated() {
        let mut s = State::new(Settings::default());
        assert!(s.saw("a".into(), "Far".into(), Some(-80)));
        assert!(s.saw("b".into(), "Near".into(), Some(-45)));
        assert!(s.saw("c".into(), "Unknown".into(), None));
        let names: Vec<_> = s.nearby.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Near", "Far", "Unknown"]);

        // Small RSSI jitter is not a visible change; a big one is and re-sorts.
        assert!(!s.saw("a".into(), "Far".into(), Some(-79)));
        assert!(s.saw("a".into(), "Far".into(), Some(-40)));
        assert_eq!(s.nearby[0].id, "a");
        assert_eq!(s.nearby.len(), 3);
    }

    #[test]
    fn json_history_is_opt_in() {
        let mut s = State::new(Settings::default());
        s.set_status(Status::Connected("Band".into()));
        s.push(m(70));
        assert!(s.to_json(false).get("history").is_none());
        let j = s.to_json(true);
        assert_eq!(j["history"][0][1], 70);
        assert_eq!(j["widgets"]["bpm"]["size"], 160);
    }
}
