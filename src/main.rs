#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod ble;
mod hr;
mod server;
mod state;
mod tray;
mod ui;

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::state::{Status, Tx};

const DEFAULT_PORT: u16 = 3030;

const HELP: &str = "\
miband-heart-rate — live heart rate from a Xiaomi Smart Band over BLE, with an OBS overlay

USAGE:
    miband-heart-rate [OPTIONS]

OPTIONS:
    --port <N>    OBS server port on 127.0.0.1 (default 3030, env MIBAND_HR_PORT)
    --hidden      start hidden in the tray
    --headless    no window or tray: BLE + OBS server only, BPM to the log
    --demo        no Bluetooth, generate a fake heart rate
    -h, --help    print this help

Logging: set RUST_LOG, e.g. RUST_LOG=debug.
";

struct Args {
    port: u16,
    hidden: bool,
    headless: bool,
    demo: bool,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        port: match std::env::var("MIBAND_HR_PORT") {
            Ok(v) => v.parse().context("MIBAND_HR_PORT must be a number")?,
            Err(_) => DEFAULT_PORT,
        },
        hidden: false,
        headless: false,
        demo: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--port" => {
                args.port = it
                    .next()
                    .context("--port needs a value")?
                    .parse()
                    .context("invalid port")?
            }
            "--hidden" => args.hidden = true,
            "--headless" => args.headless = true,
            "--demo" => args.demo = true,
            "-h" | "--help" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            other => bail!("unknown argument: {other}\n\n{HELP}"),
        }
    }
    Ok(args)
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "miband_heart_rate=info".into()),
        )
        .init();

    let args = parse_args()?;
    let rt = tokio::runtime::Runtime::new()?;
    let (tx, rx) = state::channel();

    if args.demo {
        rt.spawn(demo(tx));
    } else {
        rt.spawn(ble::run(tx));
    }

    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, args.port));
    let overlay_url = format!("http://{addr}/");
    let server_error = match rt.block_on(server::bind(addr)) {
        Ok(listener) => {
            tracing::info!("OBS overlay: {overlay_url}");
            let rx = rx.clone();
            rt.spawn(async move {
                if let Err(e) = server::serve(listener, rx).await {
                    tracing::error!("http server stopped: {e}");
                }
            });
            None
        }
        Err(e) => {
            tracing::error!("cannot listen on {addr}: {e}");
            Some(format!("port {} is in use", args.port))
        }
    };

    if args.headless {
        if server_error.is_some() {
            bail!("failed to start the OBS server on {addr}");
        }
        return rt.block_on(headless(rx));
    }

    let opts = ui::Options {
        overlay_url,
        server_error,
        start_hidden: args.hidden,
    };
    ui::run(rx, rt.handle().clone(), opts).map_err(|e| anyhow::anyhow!("{e}"))?;
    // Don't wait for BLE / HTTP tasks blocked on I/O.
    rt.shutdown_timeout(Duration::from_millis(200));
    Ok(())
}

async fn headless(mut rx: state::Rx) -> Result<()> {
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(()),
            changed = rx.changed() => {
                changed?;
                let s = rx.borrow_and_update();
                match s.bpm() {
                    Some(bpm) => tracing::info!("{bpm} BPM"),
                    None => tracing::info!("{}", s.status.label()),
                }
            }
        }
    }
}

/// Fake band for testing the UI and the OBS overlay without hardware.
async fn demo(tx: Tx) {
    tx.send_modify(|s| s.set_status(Status::Scanning));
    tokio::time::sleep(Duration::from_secs(1)).await;
    tx.send_modify(|s| s.set_status(Status::Connected("Demo Band".into())));
    let mut t = 0.0f32;
    let mut noise = 0x2545_f491u32;
    loop {
        noise ^= noise << 13;
        noise ^= noise >> 17;
        noise ^= noise << 5;
        let jitter = (noise % 5) as f32 - 2.0;
        let bpm = 78.0 + 18.0 * (t / 40.0).sin() + 6.0 * (t / 7.0).sin() + jitter;
        tx.send_modify(|s| {
            s.push(hr::Measurement {
                bpm: bpm as u16,
                contact: Some(true),
            })
        });
        t += 1.0;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
