use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakMode {
    Overlay,
    Notification,
    Both,
}

impl BreakMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            BreakMode::Overlay => "overlay",
            BreakMode::Notification => "notification",
            BreakMode::Both => "both",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub enabled: bool,
    pub work_minutes: u32,
    pub break_seconds: u32,
    pub mode: BreakMode,
    pub snooze_minutes: u32,
    pub allow_skip: bool,
    pub strict_mode: bool,
    pub sound: bool,
    pub autostart: bool,
    pub warn_before_seconds: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            work_minutes: 20,
            break_seconds: 20,
            mode: BreakMode::Overlay,
            snooze_minutes: 5,
            allow_skip: true,
            strict_mode: false,
            sound: true,
            autostart: false,
            warn_before_seconds: 0,
        }
    }
}

impl Settings {
    pub fn sanitized(mut self) -> Self {
        self.work_minutes = self.work_minutes.clamp(1, 480);
        self.break_seconds = self.break_seconds.clamp(5, 900);
        self.snooze_minutes = self.snooze_minutes.clamp(1, 60);
        self.warn_before_seconds = self.warn_before_seconds.clamp(0, 300);
        self
    }
}
