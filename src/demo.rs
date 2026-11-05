//! `--demo`: a fake Bluetooth side for trying the app and setting up OBS without a band.
//! Two bands are "nearby" so device selection can be exercised too.

use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::{interval, sleep};

use crate::ble::Command;
use crate::hr::Measurement;
use crate::state::{Status, Tx};

const DEVICES: [(&str, &str, i16); 2] = [
    ("demo-1", "Demo Band", -48),
    ("demo-2", "Someone else's band", -79),
];

pub async fn run(tx: Tx, mut cmds: UnboundedReceiver<Command>) {
    let mut paused = false;
    let mut tick = interval(Duration::from_secs(1));
    let mut t = 0.0f32;
    let mut noise = 0x2545_f491u32;
    let mut connected_to: Option<String> = None;

    loop {
        let chosen = tx.borrow().settings.device.clone();
        match (&chosen, paused) {
            (Some(d), false) => {
                if connected_to.as_deref() != Some(&d.id) {
                    tx.send_modify(|s| {
                        s.nearby.clear();
                        s.set_status(Status::Connecting(d.name.clone()));
                    });
                    sleep(Duration::from_millis(800)).await;
                    tx.send_modify(|s| s.set_status(Status::Connected(d.name.clone())));
                    connected_to = Some(d.id.clone());
                }
            }
            _ => {
                connected_to = None;
                tx.send_modify(|s| {
                    s.set_status(match &chosen {
                        Some(d) => Status::Paused(d.name.clone()),
                        None => Status::ChooseDevice,
                    });
                    for (id, name, rssi) in DEVICES {
                        s.saw(id.into(), name.into(), Some(rssi));
                    }
                });
            }
        }

        tokio::select! {
            _ = tick.tick() => {
                if connected_to.is_some() {
                    noise ^= noise << 13;
                    noise ^= noise >> 17;
                    noise ^= noise << 5;
                    let jitter = (noise % 5) as f32 - 2.0;
                    let bpm = 78.0 + 18.0 * (t / 40.0).sin() + 6.0 * (t / 7.0).sin() + jitter;
                    tx.send_modify(|s| s.push(Measurement { bpm: bpm as u16, contact: Some(true) }));
                    t += 1.0;
                }
            }
            Some(cmd) = cmds.recv() => match cmd {
                Command::Select(d) => {
                    paused = false;
                    tx.send_modify(|s| s.settings.device = Some(d));
                }
                Command::Disconnect => paused = true,
                Command::Connect => paused = false,
                Command::Forget => {
                    paused = false;
                    tx.send_modify(|s| s.settings.device = None);
                }
            },
        }
    }
}
