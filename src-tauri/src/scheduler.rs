use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use crate::overlay;
use crate::settings::{BreakMode, Settings};

pub enum Cmd {
    StartBreakNow,
    SkipBreak,
    Snooze,
    Pause(Option<u32>),
    Resume,
    SettingsChanged,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusPayload {
    pub phase: String,
    pub enabled: bool,
    pub remaining_secs: u64,
    pub total_secs: u64,
    pub next_break_in_secs: u64,
    pub paused_for_secs: Option<u64>,
    pub work_minutes: u32,
    pub break_seconds: u32,
    pub mode: String,
    pub snooze_minutes: u32,
    pub allow_skip: bool,
    pub strict_mode: bool,
    pub sound: bool,
}

pub struct Scheduler {
    tx: Mutex<Sender<Cmd>>,
    pub status: Arc<Mutex<StatusPayload>>,
}

impl Scheduler {
    pub fn send(&self, cmd: Cmd) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(cmd);
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Phase {
    Working {
        ends_at: Instant,
    },
    Breaking {
        ends_at: Instant,
    },
    Paused {
        until: Option<Instant>,
    },
}

impl Phase {
    fn is_breaking(&self) -> bool {
        matches!(self, Phase::Breaking { .. })
    }
}

fn ceil_secs(duration: Duration) -> u64 {
    duration.as_secs() + u64::from(duration.subsec_nanos() > 0)
}

fn secs_until(now: Instant, target: Instant) -> u64 {
    if target <= now {
        0
    } else {
        ceil_secs(target - now)
    }
}

fn work_duration(settings: &Settings) -> Duration {
    Duration::from_secs(settings.work_minutes as u64 * 60)
}

fn snapshot(settings: &Settings, phase: &Phase) -> StatusPayload {
    let now = Instant::now();
    let (phase_name, remaining, total, next, paused_for) = match *phase {
        Phase::Working { ends_at } => ("working", 0, 0, secs_until(now, ends_at), None),
        Phase::Breaking { ends_at } => (
            "breaking",
            secs_until(now, ends_at),
            settings.break_seconds as u64,
            0,
            None,
        ),
        Phase::Paused { until } => ("paused", 0, 0, 0, until.map(|u| secs_until(now, u))),
    };

    StatusPayload {
        phase: phase_name.to_string(),
        enabled: settings.enabled,
        remaining_secs: remaining,
        total_secs: total,
        next_break_in_secs: next,
        paused_for_secs: paused_for,
        work_minutes: settings.work_minutes,
        break_seconds: settings.break_seconds,
        mode: settings.mode.as_str().to_string(),
        snooze_minutes: settings.snooze_minutes,
        allow_skip: settings.allow_skip,
        strict_mode: settings.strict_mode,
        sound: settings.sound,
    }
}

pub fn spawn(app: AppHandle, settings: Arc<Mutex<Settings>>) -> Scheduler {
    let (tx, rx) = mpsc::channel();
    let initial = {
        let current = settings.lock().unwrap().clone();
        snapshot(
            &current,
            &Phase::Working {
                ends_at: Instant::now() + work_duration(&current),
            },
        )
    };
    let status = Arc::new(Mutex::new(initial));
    let status_thread = Arc::clone(&status);

    thread::Builder::new()
        .name("blinkwell-scheduler".to_string())
        .spawn(move || run(app, settings, status_thread, rx))
        .expect("failed to spawn scheduler thread");

    Scheduler {
        tx: Mutex::new(tx),
        status,
    }
}

fn run(
    app: AppHandle,
    settings: Arc<Mutex<Settings>>,
    status: Arc<Mutex<StatusPayload>>,
    rx: Receiver<Cmd>,
) {
    let initial_settings = settings.lock().unwrap().clone();
    let mut phase = Phase::Working {
        ends_at: Instant::now() + work_duration(&initial_settings),
    };
    let mut warned = false;
    let mut pending_overlay_close: Option<Instant> = None;
    let mut last_tick = Instant::now();
    let mut last_emit: Option<Instant> = None;
    let mut last_tooltip = Instant::now() - Duration::from_secs(60);

    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(cmd) => handle_cmd(
                &app,
                &settings,
                &status,
                &mut phase,
                &mut warned,
                &mut pending_overlay_close,
                cmd,
            ),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }

        let now = Instant::now();
        let gap = now.duration_since(last_tick);
        last_tick = now;

        // A tick gap much larger than expected means the system was
        // suspended or the process was frozen. Never fire a stale break.
        if gap > Duration::from_secs(10) {
            warned = false;
            match phase {
                Phase::Breaking { .. } => {
                    pending_overlay_close = finish_break(&app, &settings, &mut phase, false);
                }
                Phase::Working { .. } => {
                    let current = settings.lock().unwrap().clone();
                    if current.enabled {
                        phase = Phase::Working {
                            ends_at: Instant::now() + work_duration(&current),
                        };
                    }
                }
                Phase::Paused { .. } => {}
            }
        }

        let now = Instant::now();
        let mut changed = false;

        match phase {
            Phase::Working { ends_at } => {
                let current = settings.lock().unwrap().clone();
                if current.enabled && current.warn_before_seconds > 0 && !warned {
                    let warn_at = ends_at
                        .checked_sub(Duration::from_secs(current.warn_before_seconds as u64))
                        .unwrap_or(ends_at);
                    if now >= warn_at {
                        send_warning(&app, &current);
                        warned = true;
                    }
                }
                if now >= ends_at {
                    if current.enabled {
                        warned = false;
                        begin_break(&app, &settings, &mut phase);
                        changed = true;
                    } else {
                        warned = false;
                        phase = Phase::Working {
                            ends_at: now + work_duration(&current),
                        };
                    }
                }
            }
            Phase::Breaking { ends_at } => {
                if now >= ends_at {
                    pending_overlay_close = finish_break(&app, &settings, &mut phase, true);
                    changed = true;
                }
            }
            Phase::Paused { until } => {
                if let Some(deadline) = until {
                    if now >= deadline {
                        let current = settings.lock().unwrap().clone();
                        warned = false;
                        phase = Phase::Working {
                            ends_at: now + work_duration(&current),
                        };
                        changed = true;
                    }
                }
            }
        }

        if let Some(deadline) = pending_overlay_close {
            if Instant::now() >= deadline {
                overlay::close(&app);
                pending_overlay_close = None;
            }
        } else if !phase.is_breaking() && overlay::any_open(&app) {
            // Watchdog: never leave a stray overlay on screen.
            overlay::close(&app);
        }

        let now = Instant::now();
        if changed
            || last_emit.map_or(true, |last| now.duration_since(last) >= Duration::from_secs(1))
        {
            last_emit = Some(now);
            let current = settings.lock().unwrap().clone();
            let update = snapshot(&current, &phase);
            *status.lock().unwrap() = update.clone();
            let _ = app.emit("status", update);
        }

        if now.duration_since(last_tooltip) >= Duration::from_secs(5) {
            last_tooltip = now;
            update_tooltip(&app, &status);
        }
    }
}

fn handle_cmd(
    app: &AppHandle,
    settings: &Arc<Mutex<Settings>>,
    status: &Arc<Mutex<StatusPayload>>,
    phase: &mut Phase,
    warned: &mut bool,
    pending_overlay_close: &mut Option<Instant>,
    cmd: Cmd,
) {
    let mut changed = false;

    match cmd {
        Cmd::StartBreakNow => {
            *warned = false;
            begin_break(app, settings, phase);
            changed = true;
        }
        Cmd::SkipBreak => {
            if phase.is_breaking() {
                *pending_overlay_close = finish_break(app, settings, phase, false);
                *warned = false;
                changed = true;
            }
        }
        Cmd::Snooze => {
            if phase.is_breaking() {
                finish_break(app, settings, phase, false);
                let current = settings.lock().unwrap().clone();
                *phase = Phase::Working {
                    ends_at: Instant::now() + Duration::from_secs(current.snooze_minutes as u64 * 60),
                };
                *pending_overlay_close = None;
                *warned = false;
                changed = true;
            }
        }
        Cmd::Pause(minutes) => {
            if phase.is_breaking() {
                finish_break(app, settings, phase, false);
            }
            *phase = Phase::Paused {
                until: minutes.map(|m| Instant::now() + Duration::from_secs(m as u64 * 60)),
            };
            *pending_overlay_close = None;
            *warned = false;
            changed = true;
        }
        Cmd::Resume => {
            let current = settings.lock().unwrap().clone();
            *phase = Phase::Working {
                ends_at: Instant::now() + work_duration(&current),
            };
            *warned = false;
            changed = true;
        }
        Cmd::SettingsChanged => {
            let current = settings.lock().unwrap().clone();
            match phase {
                Phase::Working { .. } => {
                    *phase = Phase::Working {
                        ends_at: Instant::now() + work_duration(&current),
                    };
                    *warned = false;
                }
                Phase::Breaking { .. } => {
                    if !current.enabled {
                        *pending_overlay_close = finish_break(app, settings, phase, false);
                    }
                }
                Phase::Paused { .. } => {}
            }
            changed = true;
        }
    }

    if changed {
        let current = settings.lock().unwrap().clone();
        let update = snapshot(&current, phase);
        *status.lock().unwrap() = update.clone();
        let _ = app.emit("status", update);
    }
}

fn begin_break(app: &AppHandle, settings: &Arc<Mutex<Settings>>, phase: &mut Phase) {
    let current = settings.lock().unwrap().clone();
    let now = Instant::now();
    let overlay_mode = matches!(current.mode, BreakMode::Overlay | BreakMode::Both);
    let notify_mode = matches!(current.mode, BreakMode::Notification | BreakMode::Both);

    if notify_mode {
        send_break_notification(app, &current);
    }

    if overlay_mode {
        *phase = Phase::Breaking {
            ends_at: now + Duration::from_secs(current.break_seconds as u64),
        };
        overlay::show(app);
    } else {
        // Notification-only mode: no screen takeover, resume the work cycle
        // immediately after the notification.
        *phase = Phase::Working {
            ends_at: now + work_duration(&current),
        };
    }
}

fn finish_break(
    app: &AppHandle,
    settings: &Arc<Mutex<Settings>>,
    phase: &mut Phase,
    natural: bool,
) -> Option<Instant> {
    let current = settings.lock().unwrap().clone();
    *phase = Phase::Working {
        ends_at: Instant::now() + work_duration(&current),
    };
    let _ = app.emit("break-ended", serde_json::json!({ "natural": natural }));

    if natural && overlay::any_open(app) {
        // Give the overlays a moment to show the completion state before
        // they are closed authoritatively from here.
        Some(Instant::now() + Duration::from_millis(1600))
    } else {
        overlay::close(app);
        None
    }
}

fn send_break_notification(app: &AppHandle, settings: &Settings) {
    let _ = app
        .notification()
        .builder()
        .title("Time for an eye break")
        .body(format!(
            "Look at something 20 feet away for {} seconds. Blink slowly.",
            settings.break_seconds
        ))
        .show();
}

fn send_warning(app: &AppHandle, settings: &Settings) {
    let _ = app
        .notification()
        .builder()
        .title("Eye break coming up")
        .body(format!(
            "Your {}-second eye break starts in {} seconds.",
            settings.break_seconds, settings.warn_before_seconds
        ))
        .show();
}

fn update_tooltip(app: &AppHandle, status: &Arc<Mutex<StatusPayload>>) {
    let Some(tray) = app.tray_by_id("main-tray") else {
        return;
    };
    let snap = status.lock().unwrap();
    let text = match snap.phase.as_str() {
        "breaking" => format!("BlinkWell - eye break: {}s left", snap.remaining_secs),
        "paused" => match snap.paused_for_secs {
            Some(secs) => format!("BlinkWell - paused, resumes in {}m", secs.div_ceil(60)),
            None => "BlinkWell - paused until you resume".to_string(),
        },
        _ => {
            if !snap.enabled {
                "BlinkWell - reminders disabled".to_string()
            } else {
                format!(
                    "BlinkWell - next break in {}:{:02}",
                    snap.next_break_in_secs / 60,
                    snap.next_break_in_secs % 60
                )
            }
        }
    };
    let _ = tray.set_tooltip(Some(&text));
}
