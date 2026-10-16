use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use log::{LevelFilter, Log, Metadata, Record};

use crate::sync::lock;

/// Log files are named `blinkwell-YYYY-MM-DD.log`, with `.N.log` parts once a
/// day's file grows past the size cap. Only files matching this pattern are
/// ever rotated or removed - anything else in the folder is left strictly
/// alone.
const FILE_PREFIX: &str = "blinkwell-";
const FILE_SUFFIX: &str = ".log";
const MAX_PART_BYTES: u64 = 5 * 1024 * 1024;
const KEEP_FOR: Duration = Duration::from_secs(24 * 60 * 60);
const PRUNE_EVERY: Duration = Duration::from_secs(60 * 60);
const MAX_PARTS: u32 = 999;

struct State {
    ready: bool,
    dir: Option<PathBuf>,
    day: Option<u64>,
    part: u32,
    file: Option<File>,
    written: u64,
    current: Option<PathBuf>,
    last_prune: Option<Instant>,
}

pub struct FileLogger {
    state: Mutex<State>,
}

static LOGGER: FileLogger = FileLogger {
    state: Mutex::new(State {
        ready: false,
        dir: None,
        day: None,
        part: 0,
        file: None,
        written: 0,
        current: None,
        last_prune: None,
    }),
};

/// Start the file logger. Best effort by design: if the preferred folder is
/// unusable we fall back to the temp directory, and if even that fails the app
/// keeps running with stderr only. Never panics, never blocks for long.
pub fn init(preferred: Option<PathBuf>) {
    let dir = pick_dir(preferred);
    {
        let mut state = lock(&LOGGER.state);
        state.dir = Some(dir);
        state.ready = true;
        prune(&mut state);
    }
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(if cfg!(debug_assertions) {
            LevelFilter::Debug
        } else {
            LevelFilter::Info
        });
    }
}

/// Where the log files live, once `init` has run.
pub fn dir() -> Option<PathBuf> {
    lock(&LOGGER.state).dir.clone()
}

/// Cheap housekeeping for the scheduler to call; prunes expired files roughly
/// once an hour even when the app is otherwise idle.
pub fn tick() {
    let mut state = lock(&LOGGER.state);
    if !state.ready {
        return;
    }
    if state
        .last_prune
        .map_or(true, |last| last.elapsed() >= PRUNE_EVERY)
    {
        prune(&mut state);
    }
}

/// The newest log files within the retention window, most recent first.
pub fn recent_files(max: usize) -> Vec<PathBuf> {
    let state = lock(&LOGGER.state);
    let Some(dir) = state.dir.as_ref() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let now = SystemTime::now();
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_our_log(&path) {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else {
            continue;
        };
        let within_window = now
            .duration_since(modified)
            .map_or(true, |age| age <= KEEP_FOR);
        if within_window {
            files.push((modified, path));
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.truncate(max);
    files.into_iter().map(|(_, path)| path).collect()
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let line = format!(
            "{}.{:03}Z {:5} {} - {}\n",
            iso_timestamp(now.as_secs()),
            now.subsec_millis(),
            record.level(),
            record.target(),
            record.args()
        );

        let mut state = lock(&self.state);
        if state.ready {
            ensure_file(&mut state, now.as_secs());
            if let Some(file) = state.file.as_mut() {
                if file.write_all(line.as_bytes()).is_ok() {
                    state.written += line.len() as u64;
                }
            }
        }
        drop(state);

        #[cfg(debug_assertions)]
        eprint!("{line}");
    }

    fn flush(&self) {
        if let Some(file) = lock(&self.state).file.as_mut() {
            let _ = file.flush();
        }
    }
}

fn pick_dir(preferred: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = &preferred {
        if std::fs::create_dir_all(dir).is_ok() {
            return dir.clone();
        }
    }
    let fallback = std::env::temp_dir().join("blinkwell-logs");
    if std::fs::create_dir_all(&fallback).is_ok() {
        return fallback;
    }
    preferred.unwrap_or(fallback)
}

fn is_our_log(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.starts_with(FILE_PREFIX) && name.ends_with(FILE_SUFFIX)
}

/// Open today's file for appending, rolling to a new part when full. Opening
/// failures leave the logger in stderr-only mode and are retried on the next
/// write, so a transient lock or full disk heals by itself.
fn ensure_file(state: &mut State, secs: u64) {
    let day = secs / 86_400;
    let new_day = state.day != Some(day);
    let part_full = state.file.is_some() && state.written >= MAX_PART_BYTES;
    if state.file.is_some() && !new_day && !part_full {
        return;
    }
    let Some(dir) = state.dir.clone() else {
        return;
    };
    if new_day {
        state.day = Some(day);
        state.part = 0;
    }
    let mut part = state.part;
    while part <= MAX_PARTS {
        let path = part_path(&dir, secs, part);
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => {
                let size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
                if size >= MAX_PART_BYTES {
                    part += 1;
                    continue;
                }
                state.part = part;
                state.written = size;
                state.file = Some(file);
                state.current = Some(path);
                return;
            }
            Err(_) => return,
        }
    }
}

fn part_path(dir: &Path, secs: u64, part: u32) -> PathBuf {
    let (year, month, day, ..) = civil_from_secs(secs);
    let base = format!("{FILE_PREFIX}{year:04}-{month:02}-{day:02}");
    if part == 0 {
        dir.join(format!("{base}{FILE_SUFFIX}"))
    } else {
        dir.join(format!("{base}.{part}{FILE_SUFFIX}"))
    }
}

/// Delete our own log files that have not been touched for the retention
/// window. Only files matching our exact name pattern are ever considered,
/// and the file currently open for writing is never touched.
fn prune(state: &mut State) {
    state.last_prune = Some(Instant::now());
    let Some(dir) = state.dir.clone() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_our_log(&path) {
            continue;
        }
        if state.current.as_deref() == Some(path.as_path()) {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else {
            continue;
        };
        // A future timestamp (clock adjustment) is treated as fresh.
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age > KEEP_FOR {
            let _ = std::fs::remove_file(&path);
        }
    }
}

fn iso_timestamp(secs: u64) -> String {
    let (year, month, day, hour, minute, second) = civil_from_secs(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}")
}

/// Seconds since the Unix epoch in, UTC calendar out (Howard Hinnant's
/// days-to-civil algorithm).
fn civil_from_secs(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let day_seconds = (secs % 86_400) as u32;
    let (hour, minute, second) = (
        day_seconds / 3_600,
        (day_seconds % 3_600) / 60,
        day_seconds % 60,
    );

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day, hour, minute, second)
}
