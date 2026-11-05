//! Shared application state, published through a `tokio::sync::watch` channel.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::hr::Measurement;

/// How much history to keep for the chart and min/avg/max.
pub const HISTORY_WINDOW: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum Status {
    Starting,
    NoAdapter,
    Scanning,
    Connecting(String),
    Connected(String),
    Error(String),
}

impl Status {
    pub fn label(&self) -> String {
        match self {
            Status::Starting => "Starting…".into(),
            Status::NoAdapter => "Bluetooth unavailable".into(),
            Status::Scanning => "Searching for the band…".into(),
            Status::Connecting(name) => format!("Connecting to {name}…"),
            Status::Connected(name) => name.clone(),
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

#[derive(Debug, Clone)]
pub struct State {
    pub status: Status,
    pub last: Option<Measurement>,
    pub history: VecDeque<Sample>,
}

impl Default for State {
    fn default() -> Self {
        Self { status: Status::Starting, last: None, history: VecDeque::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Stats {
    pub min: u16,
    pub avg: u16,
    pub max: u16,
}

impl State {
    pub fn set_status(&mut self, status: Status) {
        if !status.is_connected() {
            self.last = None;
        }
        self.status = status;
    }

    pub fn push(&mut self, m: Measurement) {
        let now = Instant::now();
        self.last = Some(m);
        self.history.push_back(Sample { at: now, bpm: m.bpm });
        while self.history.front().is_some_and(|s| now.duration_since(s.at) > HISTORY_WINDOW) {
            self.history.pop_front();
        }
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
        Some(Stats { min, avg: ((sum + n / 2) / n) as u16, max })
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "bpm": self.bpm(),
            "connected": self.status.is_connected(),
            "contact": self.last.and_then(|m| m.contact),
            "status": self.status,
            "status_text": self.status.label(),
            "stats": self.stats(),
        })
    }
}

pub type Tx = tokio::sync::watch::Sender<State>;
pub type Rx = tokio::sync::watch::Receiver<State>;

pub fn channel() -> (Tx, Rx) {
    tokio::sync::watch::channel(State::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(bpm: u16) -> Measurement {
        Measurement { bpm, contact: Some(true) }
    }

    #[test]
    fn stats_and_bpm_visibility() {
        let mut s = State::default();
        assert_eq!(s.stats(), None);
        s.set_status(Status::Connected("Band".into()));
        for b in [60, 70, 81] {
            s.push(m(b));
        }
        assert_eq!(s.bpm(), Some(81));
        assert_eq!(s.stats(), Some(Stats { min: 60, avg: 70, max: 81 }));

        s.set_status(Status::Scanning);
        assert_eq!(s.bpm(), None);
        // history survives a reconnect
        assert_eq!(s.stats().unwrap().max, 81);
    }
}
