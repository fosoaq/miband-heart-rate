//! Finds a device advertising the Heart Rate service, subscribes to
//! Heart Rate Measurement notifications and keeps reconnecting forever.

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use bluest::{Adapter, Device, Uuid, btuuid::bluetooth_uuid_from_u16};
use futures_lite::StreamExt;
use tokio::time::{sleep, timeout};

use crate::hr;
use crate::state::{Status, Tx};

const HEART_RATE_SERVICE: Uuid = bluetooth_uuid_from_u16(0x180D);
const HEART_RATE_MEASUREMENT: Uuid = bluetooth_uuid_from_u16(0x2A37);

/// The band sends roughly one notification per second; silence this long means it's gone.
const NOTIFY_TIMEOUT: Duration = Duration::from_secs(15);
const RETRY_DELAY: Duration = Duration::from_secs(3);

pub async fn run(tx: Tx) {
    loop {
        let Some(adapter) = Adapter::default().await else {
            tx.send_modify(|s| s.set_status(Status::NoAdapter));
            sleep(Duration::from_secs(5)).await;
            continue;
        };
        if let Err(e) = adapter.wait_available().await {
            tracing::warn!("adapter unavailable: {e}");
            tx.send_modify(|s| s.set_status(Status::NoAdapter));
            sleep(Duration::from_secs(5)).await;
            continue;
        }

        loop {
            let err = match session(&adapter, &tx).await {
                Ok(()) => anyhow!("Lost connection to the band"),
                Err(e) => e,
            };
            tracing::warn!("{err:#}");
            tx.send_modify(|s| s.set_status(Status::Error(format!("{err}"))));
            sleep(RETRY_DELAY).await;
        }
    }
}

/// One scan → connect → stream cycle. Returns when the stream ends.
async fn session(adapter: &Adapter, tx: &Tx) -> Result<()> {
    let device = find_device(adapter, tx).await?;
    let name = device.name_async().await.unwrap_or_else(|_| "band".into());

    if !device.is_connected().await {
        tx.send_modify(|s| s.set_status(Status::Connecting(name.clone())));
        tracing::info!("connecting to {name} ({})", device.id());
        adapter.connect_device(&device).await.context("Failed to connect")?;
    }

    let result = stream_measurements(&device, &name, tx).await;
    // Best effort: leave the band in a clean state for the next attempt.
    let _ = adapter.disconnect_device(&device).await;
    result
}

async fn find_device(adapter: &Adapter, tx: &Tx) -> Result<Device> {
    // The band may already be connected to this computer (e.g. by the OS).
    let connected = adapter.connected_devices_with_services(&[HEART_RATE_SERVICE]).await?;
    if let Some(device) = connected.into_iter().next() {
        return Ok(device);
    }

    tx.send_modify(|s| s.set_status(Status::Scanning));
    tracing::info!("scanning for heart rate broadcast");
    let mut scan = adapter.discover_devices(&[HEART_RATE_SERVICE]).await?;
    let device = scan.next().await.ok_or_else(|| anyhow!("Scan stopped"))??;
    Ok(device)
}

async fn stream_measurements(device: &Device, name: &str, tx: &Tx) -> Result<()> {
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

    let mut updates = characteristic.notify().await.context("Failed to subscribe to heart rate notifications")?;
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
