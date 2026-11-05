#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod ble;
mod demo;
mod hr;
mod server;
mod settings;
mod state;
mod tray;
mod ui;

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use anyhow::{Context, Result, bail};

const DEFAULT_PORT: u16 = 3030;

const HELP: &str = "\
miband-heart-rate — live heart rate from a Xiaomi Smart Band over BLE, with OBS widgets

USAGE:
    miband-heart-rate [OPTIONS]

OPTIONS:
    --port <N>    OBS server port on 127.0.0.1 (default 3030, env MIBAND_HR_PORT)
    --hidden      start hidden in the tray
    --headless    no window or tray: BLE + OBS server only, BPM to the log.
                  Without a saved band, uses the one with the strongest signal.
    --demo        no Bluetooth, fake bands and heart rate (for setting up OBS)
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

    let mut loaded = settings::load();
    if let Some(path) = settings::path() {
        tracing::info!("settings: {}", path.display());
    }
    // The demo starts with no band chosen and never saves one over the real choice.
    let pinned_device = args.demo.then(|| loaded.device.take());

    let (tx, rx) = state::channel(loaded);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();

    rt.spawn(settings::persist(rx.clone(), pinned_device));
    if args.demo {
        rt.spawn(demo::run(tx.clone(), cmd_rx));
    } else {
        let opts = ble::Options {
            auto_pick: args.headless,
        };
        rt.spawn(ble::run(tx.clone(), cmd_rx, opts));
    }

    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, args.port));
    let base_url = format!("http://{addr}");
    let server_error = match rt.block_on(server::bind(addr)) {
        Ok(listener) => {
            for kind in server::WIDGETS {
                tracing::info!("OBS widget: {base_url}/widget/{kind}");
            }
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
        // Keep the command channel open so the BLE task doesn't treat it as closed.
        let _cmds = cmd_tx;
        return rt.block_on(headless(rx));
    }

    let opts = ui::Options {
        base_url,
        server_error,
        start_hidden: args.hidden,
    };
    ui::run(tx, cmd_tx, rt.handle().clone(), opts).map_err(|e| anyhow::anyhow!("{e}"))?;
    // Give a pending settings save a moment, but don't wait for BLE / HTTP tasks blocked on I/O.
    rt.shutdown_timeout(Duration::from_millis(700));
    Ok(())
}

async fn headless(mut rx: state::Rx) -> Result<()> {
    let mut last = String::new();
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(()),
            changed = rx.changed() => {
                changed?;
                let line = {
                    let s = rx.borrow_and_update();
                    match s.bpm() {
                        Some(bpm) => format!("{bpm} BPM"),
                        None => s.status.label(),
                    }
                };
                // The state also changes for the nearby list; only log what's new.
                if line != last {
                    tracing::info!("{line}");
                    last = line;
                }
            }
        }
    }
}
