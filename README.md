# BlinkWell

BlinkWell is a break reminder for Windows and Linux that follows the 20-20-20 rule: every 20 minutes, look at something 20 feet away for 20 seconds.

It lives in the system tray and stays out of the way until it's time for a break. The default experience is a full-screen countdown on every monitor, because gentle nudges are easy to ignore and your eyes don't get their break. If that's too much, notification-only mode is one switch away. There's always a skip button for the moments you genuinely can't stop.

## Features

- Configurable work interval (1-180 min) and break length (5-120 s), 20/20 by default
- Full-screen break overlay on every monitor with a countdown ring, snooze, and skip
- Notification-only mode, or notifications and the overlay together
- Accurate, drift-free timers: monotonic clock, suspend/resume aware, and a watchdog that never leaves an overlay stranded on screen
- Tray menu with the next break countdown in the tooltip, "take a break now", and pause for 15/30/60 minutes or indefinitely
- Strict skip: hold the skip button for 3 seconds so breaks can't be dismissed by a stray click
- Optional chime, pre-break warning, and start-at-login
- Settings persist to disk, single instance enforced
- Light on resources: ~2 MB installers, ~30 MB of RAM while idle

## Install

Grab an installer from the [releases page](https://github.com/akhlashashmi/blinkwell/releases).

- `BlinkWell_x.x.x_x64-setup.exe` - Windows installer
- `BlinkWell_x.x.x_x64_en-US.msi` - same app, for managed deployment (GPO, Intune, SCCM)

There are no Linux binaries published yet; it builds from source there and the same command produces `.deb` and `.AppImage` packages.

## Usage

The app runs in the tray.

- Left-click the tray icon to open settings
- Right-click for the menu: take a break now, pause, resume, quit
- Closing the settings window doesn't quit - BlinkWell keeps running in the tray. Use Quit in the tray menu or the settings window to exit.

The settings window shows a live countdown, so you can always see when the next break lands.

## Settings

| Setting | Default | Notes |
| --- | --- | --- |
| Reminders | On | Master switch |
| Work interval | 20 min | Time between breaks |
| Break length | 20 s | Length of the overlay countdown |
| Reminder style | Overlay | Overlay, notification, or both |
| Snooze length | 5 min | Delay when you hit snooze |
| Warn before break | Off | Heads-up notification (10/30/60 s) |
| Sound | On | Soft chime at break start and end |
| Allow skipping | On | Show the skip button on the overlay |
| Strict skip | Off | Hold-to-skip instead of a single click |
| Start at login | Off | Launch at login, minimized to the tray |

## Building from source

You need Node 20+, a stable Rust toolchain, and the usual platform dependencies (MSVC build tools on Windows; `webkit2gtk` and friends on Linux - see the [Tauri prerequisites](https://tauri.app/start/prerequisites/)).

```sh
npm install
npm run tauri dev     # run in development
npm run tauri build   # build installers
```

Installers end up in `src-tauri/target/release/bundle/` unless a custom `CARGO_TARGET_DIR` is set.

## How it works

BlinkWell is a Tauri 2 app with a small Rust core and a plain TypeScript front end - no framework, no runtime dependencies.

The scheduler is a dedicated thread ticking every 200 ms against monotonic `Instant`s, so timing doesn't drift and isn't affected by wall-clock adjustments. Large tick gaps (system sleep, hibernation, a frozen process) are treated as a wake-up: the current interval restarts instead of firing a stale break. Overlay windows are created per monitor when a break starts and are closed by the scheduler itself; a watchdog sweeps any leftover window so you're never stuck staring at an overlay.

Settings are stored as JSON in the app data directory.
