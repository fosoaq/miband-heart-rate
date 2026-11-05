# MiBand Heart Rate

Live heart rate from a Xiaomi Smart Band on your desktop: a tray app for Windows, macOS
and Linux, with a ready-made overlay for OBS.

- Reads the band's heart rate broadcast over Bluetooth LE and reconnects on its own.
- Shows the current BPM in the macOS menu bar or the tray tooltip.
- A small window with the current reading, min / avg / max and a 10-minute chart.
- Serves a transparent overlay for an OBS Browser source, plus a JSON API.

Tested with Xiaomi Smart Band 10. It uses the standard Bluetooth Heart Rate service
(`0x180D`), so other bands and chest straps that broadcast it should work too.
For Mi Band 4–7 see the upstream [`miband-4-to-7`](https://github.com/Tnze/miband-heart-rate/tree/miband-4-to-7) tag.

## Install

Download the archive for your system from [Releases](../../releases).

**macOS** — unzip and move `MiBand Heart Rate.app` to Applications. The app is not
notarized, so the first launch needs right-click → **Open** (or
`xattr -dr com.apple.quarantine "/Applications/MiBand Heart Rate.app"`). Allow Bluetooth
access when asked.

**Windows** — unzip and run `miband-heart-rate.exe`. SmartScreen may warn about an
unknown publisher: **More info** → **Run anyway**. Windows 10 or later.

**Linux** — unpack and run `miband-heart-rate`. Needs BlueZ running. The tray icon uses
StatusNotifierItem: it works out of the box on KDE and most desktops; on GNOME install
the *AppIndicator and KStatusNotifierItem Support* extension.

## Usage

1. On the band, turn on heart rate sharing (the *Share HR* / heart rate broadcast
   setting).
2. Start the app. It finds the band and connects; the BPM appears in the window and the
   tray.
3. Closing the window hides it to the tray. Use the tray menu to bring it back or quit.

### Command line

```
miband-heart-rate [OPTIONS]

  --port <N>    OBS server port on 127.0.0.1 (default 3030, env MIBAND_HR_PORT)
  --hidden      start hidden in the tray
  --headless    no window or tray: Bluetooth + OBS server only, BPM to the log
  --demo        no Bluetooth, generate a fake heart rate (for setting up OBS)
```

Logging is controlled by `RUST_LOG`, e.g. `RUST_LOG=debug`.

## OBS overlay

Add a **Browser** source with the URL `http://127.0.0.1:3030/`. The background is
transparent; the number updates the moment a reading arrives and the heart beats at the
current rate.

The look can be tuned with query parameters:

| Parameter | Default | Meaning |
|---|---|---|
| `color` | `ff4d5e` | heart colour, hex without `#` |
| `size` | `160` | font size of the number, px |
| `label` | `1` | show `BPM` under the number |
| `status` | `1` | show the connection status while there is no reading |

Example: `http://127.0.0.1:3030/?color=ffffff&size=120&label=0`

The server only listens on `127.0.0.1` and only answers requests addressed to
`127.0.0.1` or `localhost`.

### API

`GET /api` returns the current state; `GET /events` streams the same object as
Server-Sent Events on every change.

```json
{
  "bpm": 72,
  "connected": true,
  "contact": true,
  "status": { "state": "connected", "detail": "Xiaomi Smart Band 10 DD4E" },
  "status_text": "Xiaomi Smart Band 10 DD4E",
  "stats": { "min": 64, "avg": 71, "max": 88 }
}
```

`bpm` is `null` while disconnected. `contact` is whether the band is worn, `null` if
the band doesn't report it. `stats` covers the last 10 minutes.

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
