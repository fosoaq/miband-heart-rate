//! Scans for bands broadcasting heart rate, connects to the one the user chose
//! and streams Heart Rate Measurement notifications. Reconnects forever.
//!
//! The user drives it with [`Command`]s: pick a band, disconnect, reconnect, forget.

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use bluest::{Adapter, AdvertisingDevice, Device, Uuid, btuuid::bluetooth_uuid_from_u16};
use futures_lite::StreamExt;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::{Instant, interval, sleep, sleep_until, timeout};

use crate::hr;
use crate::settings::SavedDevice;
use crate::state::{Status, Tx};

const HEART_RATE_SERVICE: Uuid = bluetooth_uuid_from_u16(0x180D);
const HEART_RATE_MEASUREMENT: Uuid = bluetooth_uuid_from_u16(0x2A37);

/// The band sends roughly one notification per second; silence this long means it's gone.
const NOTIFY_TIMEOUT: Duration = Duration::from_secs(15);
const RETRY_DELAY: Duration = Duration::from_secs(3);
/// Headless mode without a saved band: listen this long, then take the strongest signal.
const AUTO_PICK_AFTER: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Use this band from now on (and connect to it).
    Select(SavedDevice),
    /// Drop the connection but keep the band chosen.
    Disconnect,
    /// Reconnect to the chosen band after [`Command::Disconnect`].
    Connect,
    /// Forget the chosen band.
    Forget,
}

pub struct Options {
    /// With no band chosen, pick the strongest one by itself (for `--headless`).
    pub auto_pick: bool,
}

/// What to do next, decided from the saved band and the pause flag.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Goal {
    Choose,
    Paused(SavedDevice),
    Find(SavedDevice),
}

struct Ctl {
    tx: Tx,
    cmds: UnboundedReceiver<Command>,
    paused: bool,
    opts: Options,
}

impl Ctl {
    fn goal(&self) -> Goal {
        match (self.tx.borrow().settings.device.clone(), self.paused) {
            (None, _) => Goal::Choose,
            (Some(d), true) => Goal::Paused(d),
            (Some(d), false) => Goal::Find(d),
        }
    }

    fn apply(&mut self, cmd: Command) {
        tracing::info!("command: {cmd:?}");
        match cmd {
            Command::Select(d) => {
                self.paused = false;
                self.tx.send_modify(|s| s.settings.device = Some(d));
            }
            Command::Disconnect => self.paused = true,
            Command::Connect => self.paused = false,
            Command::Forget => {
                self.paused = false;
                self.tx.send_modify(|s| s.settings.device = None);
            }
        }
    }

    async fn next_command(&mut self) -> Command {
        match self.cmds.recv().await {
            Some(c) => c,
            // All senders gone (UI closed): nothing will ever change the goal again.
            None => std::future::pending().await,
        }
    }
}

pub async fn run(tx: Tx, cmds: UnboundedReceiver<Command>, opts: Options) {
    let mut ctl = Ctl {
        tx,
        cmds,
        paused: false,
        opts,
    };
    loop {
        let Some(adapter) = Adapter::default().await else {
            ctl.tx.send_modify(|s| s.set_status(Status::NoAdapter));
            sleep(Duration::from_secs(5)).await;
            continue;
        };
        if let Err(e) = adapter.wait_available().await {
            tracing::warn!("adapter unavailable: {e}");
            ctl.tx.send_modify(|s| s.set_status(Status::NoAdapter));
            sleep(Duration::from_secs(5)).await;
            continue;
        }
        if let Err(e) = drive(&adapter, &mut ctl).await {
            tracing::warn!("bluetooth: {e:#}");
            ctl.tx
                .send_modify(|s| s.set_status(Status::Error(format!("{e}"))));
            sleep(RETRY_DELAY).await;
        }
    }
}

/// Scan → connect → stream, reacting to commands at every step.
async fn drive(adapter: &Adapter, ctl: &mut Ctl) -> Result<()> {
    loop {
        let goal = ctl.goal();
        ctl.tx.send_modify(|s| {
            s.set_status(match &goal {
                Goal::Choose => Status::ChooseDevice,
                Goal::Paused(d) => Status::Paused(d.name.clone()),
                Goal::Find(d) => Status::Searching(d.name.clone()),
            })
        });

        // Scan until the chosen band shows up (or forever, listing devices, while choosing/paused).
        let tx = ctl.tx.clone();
        let auto_pick = ctl.opts.auto_pick;
        let found = tokio::select! {
            r = scan(adapter, &tx, &goal, auto_pick) => r?,
            cmd = ctl.next_command() => { ctl.apply(cmd); continue; }
        };
        let (device, saved) = found;
        // No scanning while connected, so the list would only go stale.
        ctl.tx
            .send_if_modified(|s| !std::mem::take(&mut s.nearby).is_empty());

        let result = tokio::select! {
            r = session(adapter, &device, &saved.name, &tx) => Some(r),
            cmd = ctl.next_command() => { ctl.apply(cmd); None }
        };
        // Best effort: leave the band free for the next attempt or another app.
        let _ = adapter.disconnect_device(&device).await;

        if let Some(r) = result {
            let err = r
                .err()
                .unwrap_or_else(|| anyhow!("Lost connection to the band"));
            tracing::warn!("{err:#}");
            ctl.tx
                .send_modify(|s| s.set_status(Status::Error(format!("{err}"))));
            tokio::select! {
                _ = sleep(RETRY_DELAY) => {}
                cmd = ctl.next_command() => ctl.apply(cmd),
            }
        }
    }
}

/// Keeps the nearby list fresh. Returns once the wanted band is seen
/// (or, with `auto_pick` and nothing chosen, once the strongest band is clear).
async fn scan(
    adapter: &Adapter,
    tx: &Tx,
    goal: &Goal,
    auto_pick: bool,
) -> Result<(Device, SavedDevice)> {
    // Bands already connected to this computer don't advertise; list them first.
    for device in adapter
        .connected_devices_with_services(&[HEART_RATE_SERVICE])
        .await?
    {
        let id = device.id().to_string();
        let name = device_name(&device, None).await;
        tx.send_if_modified(|s| s.saw(id.clone(), name.clone(), None));
        if let Goal::Find(want) = goal
            && want.id == id
        {
            return Ok((device, SavedDevice { id, name }));
        }
    }

    tracing::info!("scanning ({goal:?})");
    let mut ads = adapter.scan(&[HEART_RATE_SERVICE]).await?;
    let mut prune = interval(Duration::from_secs(2));
    let pick_at = Instant::now() + AUTO_PICK_AFTER;
    let auto_pick = auto_pick && *goal == Goal::Choose;
    let mut best: Option<(i16, Device, SavedDevice)> = None;

    loop {
        tokio::select! {
            ad = ads.next() => {
                let AdvertisingDevice { device, adv_data, rssi } =
                    ad.ok_or_else(|| anyhow!("Scan stopped"))?;
                let id = device.id().to_string();
                let name = device_name(&device, adv_data.local_name.as_deref()).await;
                tracing::debug!(%id, %name, ?rssi, "advertisement");
                tx.send_if_modified(|s| s.saw(id.clone(), name.clone(), rssi));

                if let Goal::Find(want) = goal
                    && want.id == id
                {
                    return Ok((device, SavedDevice { id, name }));
                }
                if auto_pick {
                    let r = rssi.unwrap_or(i16::MIN);
                    if best.as_ref().is_none_or(|(b, ..)| r > *b) {
                        best = Some((r, device, SavedDevice { id, name }));
                    }
                }
            }
            _ = prune.tick() => {
                tx.send_if_modified(|s| s.prune_nearby());
            }
            _ = sleep_until(pick_at), if auto_pick && best.is_some() => {
                let (_, device, saved) = best.take().expect("checked");
                tracing::info!("picked {} ({}) — strongest signal", saved.name, saved.id);
                tx.send_modify(|s| s.settings.device = Some(saved.clone()));
                return Ok((device, saved));
            }
        }
    }
}

/// The advertised name is often shortened ("Xiaomi "); prefer the longer, trimmed one.
async fn device_name(device: &Device, advertised: Option<&str>) -> String {
    let stored = device.name_async().await.ok();
    [advertised.map(str::to_owned), stored]
        .into_iter()
        .flatten()
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .max_by_key(|n| n.len())
        .unwrap_or_else(|| "Unnamed device".into())
}

async fn session(adapter: &Adapter, device: &Device, name: &str, tx: &Tx) -> Result<()> {
    if !device.is_connected().await {
        tx.send_modify(|s| s.set_status(Status::Connecting(name.to_owned())));
        tracing::info!("connecting to {name} ({})", device.id());
        adapter
            .connect_device(device)
            .await
            .context("Failed to connect")?;
    }

    let service = device
        .discover_services_with_uuid(HEART_RATE_SERVICE)
        .await?
        .into_iter()
        .next()
        .context("Device has no heart rate service")?;
    let characteristic = service
        .discover_characteristics_with_uuid(HEART_RATE_MEASUREMENT)
        .await?
        .into_iter()
        .next()
        .context("Device has no heart rate measurement characteristic")?;

    let mut updates = characteristic
        .notify()
        .await
        .context("Failed to subscribe to heart rate notifications")?;
    tx.send_modify(|s| s.set_status(Status::Connected(name.to_owned())));
    tracing::info!("connected to {name}");

    loop {
        let Ok(next) = timeout(NOTIFY_TIMEOUT, updates.next()).await else {
            return Err(anyhow!("Band stopped sending heart rate"));
        };
        let Some(data) = next.transpose()? else {
            return Ok(());
        };
        match hr::parse(&data) {
            Some(m) if m.bpm > 0 => {
                tracing::debug!(bpm = m.bpm, contact = ?m.contact);
                tx.send_modify(|s| s.push(m));
            }
            Some(_) => {}
            None => tracing::debug!("malformed measurement: {data:02x?}"),
        }
    }
}
