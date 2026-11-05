# MiBand Heart Rate

Live heart rate from a Xiaomi Smart Band on your desktop: a tray app for Windows, macOS
and Linux, with customizable widgets for OBS.

- Reads the band's heart rate broadcast over Bluetooth LE. You pick your band once;
  after that it connects only to that one and reconnects on its own.
- Shows the current BPM in the macOS menu bar or the tray tooltip.
- A small window with the current reading, min / avg / max and a 10-minute chart.
- Three transparent widgets for OBS Browser sources (heart rate, chart, min / avg / max),
  styled from the app and updated live, plus a JSON API.

Tested with Xiaomi Smart Band 10. It uses the standard Bluetooth Heart Rate service
(`0x180D`), so other bands and chest straps that broadcast it should work too.
For Mi Band 4–7 see the upstream [`miband-4-to-7`](https://github.com/Tnze/miband-heart-rate/tree/miband-4-to-7) tag.

## Install

Download the file for your system from [Releases](../../releases).

**macOS** — `miband-heart-rate-macos-universal.zip` (Apple silicon and Intel). Unzip and
move `MiBand Heart Rate.app` to Applications. The app is not notarized, so the first
launch needs right-click → **Open** (or
`xattr -dr com.apple.quarantine "/Applications/MiBand Heart Rate.app"`). Allow Bluetooth
access when asked.

**Windows** — `miband-heart-rate-windows-x86_64.exe`, nothing to install: just run it.
SmartScreen may warn about an unknown publisher: **More info** → **Run anyway**.
Windows 10 or later.

**Linux** (x86_64, glibc 2.35+ — Ubuntu 22.04, Debian 12, Fedora 36 and newer):

- Debian / Ubuntu: `sudo apt install ./miband-heart-rate-linux-amd64.deb` — adds the
  app to the menu.
- Any distribution: `miband-heart-rate-linux-x86_64.AppImage`, then
  `chmod +x` it and run.
- Or the bare binary in `miband-heart-rate-linux-x86_64.tar.gz`.

Needs BlueZ running. The tray icon uses StatusNotifierItem: it works out of the box on
KDE and most desktops; on GNOME install the *AppIndicator and KStatusNotifierItem
Support* extension.

Each release also lists `SHA256SUMS` for checking the downloads.

## Usage

1. On the band, turn on heart rate sharing (the *Share HR* / heart rate broadcast
   setting).
2. Start the app. The **Device** tab lists the bands broadcasting nearby, strongest
   signal first; click **use** next to yours. If you're not sure which one it is, hold
   the band next to the computer and it moves to the top.
3. From then on the app connects only to that band, also after a restart. **disconnect**
   pauses it, **forget** lets you choose again.
4. Closing the window hides it to the tray. Use the tray menu to bring it back or quit.

Settings (the chosen band and the widget styles) are saved in `settings.json` in the
system config folder: `~/Library/Application Support/miband-heart-rate` on macOS,
`%APPDATA%\miband-heart-rate` on Windows, `~/.config/miband-heart-rate` on Linux.

### Command line

```
miband-heart-rate [OPTIONS]

  --port <N>    OBS server port on 127.0.0.1 (default 3030, env MIBAND_HR_PORT)
  --hidden      start hidden in the tray
  --headless    no window or tray: Bluetooth + OBS server only, BPM to the log.
                Without a saved band, uses the one with the strongest signal.
  --demo        no Bluetooth, fake bands and heart rate (for setting up OBS)
```

`--demo` never replaces the band you chose; widget styles changed in the demo are kept.

Logging is controlled by `RUST_LOG`, e.g. `RUST_LOG=debug`.

## OBS widgets

| Widget | URL |
|---|---|
| Heart rate (number + beating heart) | `http://127.0.0.1:3030/widget/bpm` |
| Chart (last 1–10 minutes) | `http://127.0.0.1:3030/widget/chart` |
| Min / avg / max | `http://127.0.0.1:3030/widget/stats` |

Add each one you want as a separate **Browser** source, so they can be placed and scaled
independently. The background is transparent.

Everything about their look is set in the app's **Overlay** tab: sizes, colours (with
transparency), what to show, the chart's time span, line and labels, row or column layout
for the stats. Changes show up in OBS immediately, without refreshing the source.

`http://127.0.0.1:3030/` is the heart rate widget too, so older setups keep working.

The server only listens on `127.0.0.1` and only answers requests addressed to
`127.0.0.1` or `localhost`.

### API

`GET /api` returns the current state; `GET /events` streams the same object as
Server-Sent Events on every change. Add `?history=1` to either to include the samples.

```json
{
  "bpm": 72,
  "connected": true,
  "contact": true,
  "status": { "state": "connected", "detail": "Xiaomi Smart Band 10 DD4E" },
  "status_text": "Xiaomi Smart Band 10 DD4E",
  "stats": { "min": 64, "avg": 71, "max": 88 },
  "widgets": { "bpm": { "size": 160, "color": "#ffffffff", "...": "..." }, "chart": {}, "stats": {} },
  "history": [[598.0, 66], [597.0, 67], "..."]
}
```

`bpm` is `null` while disconnected. `contact` is whether the band is worn, `null` if
the band doesn't report it. `stats` covers the last 10 minutes. `history` (only with
`?history=1`) is `[seconds_ago, bpm]` pairs, oldest first. `widgets` holds the styles
from the Overlay tab.

## Building from source

Rust 1.95 or later.

```sh
cargo build --release
```

On Linux install the D-Bus headers first: `sudo apt install pkg-config libdbus-1-dev`
(Debian/Ubuntu) or `sudo dnf install pkgconf-pkg-config dbus-devel` (Fedora).

On macOS, a binary started from a terminal uses that terminal's Bluetooth permission.
To build the app bundle: `scripts/bundle-macos.sh target/release/miband-heart-rate 0.2.0 dist`.

## Credits

Based on [Tnze/miband-heart-rate](https://github.com/Tnze/miband-heart-rate), which
worked out reading the band's broadcast. Bluetooth via
[bluest](https://github.com/alexmoon/bluest), UI via [egui](https://github.com/emilk/egui).

## License

MIT, see [LICENSE](LICENSE).
