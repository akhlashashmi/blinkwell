import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type BreakMode = "overlay" | "notification" | "both";

export interface Settings {
  enabled: boolean;
  workMinutes: number;
  breakSeconds: number;
  mode: BreakMode;
  snoozeMinutes: number;
  allowSkip: boolean;
  strictMode: boolean;
  sound: boolean;
  autostart: boolean;
  warnBeforeSeconds: number;
}

export interface Status {
  phase: "working" | "breaking" | "paused";
  enabled: boolean;
  remainingSecs: number;
  totalSecs: number;
  nextBreakInSecs: number;
  pausedForSecs: number | null;
  workMinutes: number;
  breakSeconds: number;
  mode: BreakMode;
  snoozeMinutes: number;
  allowSkip: boolean;
  strictMode: boolean;
  sound: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  enabled: true,
  workMinutes: 20,
  breakSeconds: 20,
  mode: "overlay",
  snoozeMinutes: 5,
  allowSkip: true,
  strictMode: false,
  sound: true,
  autostart: false,
  warnBeforeSeconds: 0,
};

export const getSettings = () => invoke<Settings>("get_settings");

export const saveSettings = (settings: Settings) =>
  invoke<Settings>("save_settings", { newSettings: settings });

export const getStatus = () => invoke<Status>("get_status");

export const startBreakNow = () => invoke("start_break_now");

export const skipBreak = () => invoke("skip_break");

export const snoozeBreak = () => invoke("snooze_break");

export const pauseReminders = (minutes: number | null) =>
  invoke("pause_reminders", { minutes });

export const resumeReminders = () => invoke("resume_reminders");

export const quitApp = () => invoke("quit_app");

export const reportCrash = () => invoke<string>("report_crash");

export const openLogsFolder = () => invoke<string>("open_logs_folder");

export function onStatus(callback: (status: Status) => void): Promise<UnlistenFn> {
  return listen<Status>("status", (event) => callback(event.payload));
}

export function onBreakEnded(
  callback: (payload: { natural: boolean }) => void,
): Promise<UnlistenFn> {
  return listen<{ natural: boolean }>("break-ended", (event) =>
    callback(event.payload),
  );
}

export function formatClock(totalSeconds: number): string {
  const safe = Math.max(0, Math.floor(totalSeconds));
  const minutes = Math.floor(safe / 60);
  const seconds = safe % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}
