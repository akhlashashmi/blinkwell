import "./styles.css";
import {
  DEFAULT_SETTINGS,
  formatClock,
  getSettings,
  getStatus,
  onStatus,
  openLogsFolder,
  pauseReminders,
  quitApp,
  reportCrash,
  resumeReminders,
  saveSettings,
  startBreakNow,
  type BreakMode,
  type Settings,
  type Status,
} from "./lib";

const MODE_HINTS: Record<BreakMode, string> = {
  overlay:
    "A black full-screen countdown on every monitor. The strongest way to actually rest your eyes, recommended for the 20-20-20 rule.",
  notification:
    "A system notification nudges you, without taking over the screen.",
  both: "A system notification plus the full-screen countdown for maximum effect.",
};

function el<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (!found) throw new Error(`Missing element #${id}`);
  return found as T;
}

const input = (id: string) => el<HTMLInputElement>(id);
const button = (id: string) => el<HTMLButtonElement>(id);

let loaded = false;
let saveTimer: number | undefined;
let currentMode: BreakMode = "overlay";
let status: Status | null = null;

function setMode(mode: BreakMode) {
  currentMode = mode;
  document.querySelectorAll<HTMLButtonElement>("[data-mode]").forEach((btn) => {
    btn.setAttribute("aria-pressed", String(btn.dataset.mode === mode));
  });
  el("mode-hint").textContent = MODE_HINTS[mode];
}

function applySettingsToForm(settings: Settings) {
  input("enabled").checked = settings.enabled;
  input("work-minutes").value = String(settings.workMinutes);
  input("break-seconds").value = String(settings.breakSeconds);
  input("snooze-minutes").value = String(settings.snoozeMinutes);
  input("warn-before").value = String(settings.warnBeforeSeconds);
  input("sound").checked = settings.sound;
  input("allow-skip").checked = settings.allowSkip;
  input("strict-mode").checked = settings.strictMode;
  input("autostart").checked = settings.autostart;
  setMode(settings.mode);
  refreshOutputs();
  syncStrictAvailability();
}

function readForm(): Settings {
  return {
    enabled: input("enabled").checked,
    workMinutes: Number(input("work-minutes").value),
    breakSeconds: Number(input("break-seconds").value),
    mode: currentMode,
    snoozeMinutes: Number(input("snooze-minutes").value),
    allowSkip: input("allow-skip").checked,
    strictMode: input("strict-mode").checked,
    sound: input("sound").checked,
    autostart: input("autostart").checked,
    warnBeforeSeconds: Number(input("warn-before").value),
  };
}

function refreshOutputs() {
  el("work-minutes-out").textContent = `${input("work-minutes").value} min`;
  el("break-seconds-out").textContent = `${input("break-seconds").value} s`;
  el("snooze-minutes-out").textContent = `${input("snooze-minutes").value} min`;
}

function syncStrictAvailability() {
  input("strict-mode").disabled = !input("allow-skip").checked;
}

function flashSaved() {
  const indicator = el("save-state");
  indicator.textContent = "Saved";
  indicator.classList.remove("pulse");
  void indicator.offsetWidth;
  indicator.classList.add("pulse");
}

function scheduleSave() {
  if (!loaded) return;
  window.clearTimeout(saveTimer);
  el("save-state").textContent = "Saving";
  saveTimer = window.setTimeout(() => {
    void saveSettings(readForm())
      .then((saved) => {
        setMode(saved.mode);
        flashSaved();
      })
      .catch(() => {
        el("save-state").textContent = "Could not save";
      });
  }, 400);
}

function setReportState(message: string, kind: "info" | "error" = "info") {
  const state = el("report-state");
  state.textContent = message;
  state.dataset.kind = kind;
}

async function reportCrashFlow() {
  const btn = button("btn-report");
  btn.disabled = true;
  setReportState("Opening your email app...");
  try {
    setReportState(await reportCrash());
  } catch (error) {
    setReportState(String(error), "error");
  } finally {
    btn.disabled = false;
  }
}

async function openLogsFolderFlow() {
  try {
    await openLogsFolder();
    setReportState("Logs folder opened.");
  } catch (error) {
    setReportState(String(error), "error");
  }
}

function renderStatus() {
  const dot = el("status-dot");
  const label = el("status-label");
  const heroCount = el("hero-count");
  document.body.dataset.phase = !status
    ? "working"
    : !status.enabled
      ? "off"
      : status.phase;

  if (!status) {
    dot.dataset.state = "working";
    label.textContent = "Connecting...";
    heroCount.textContent = "--:--";
    return;
  }

  if (!status.enabled) {
    dot.dataset.state = "off";
    label.textContent = "Reminders are off";
    heroCount.textContent = "Off";
    return;
  }

  if (status.phase === "breaking") {
    dot.dataset.state = "breaking";
    label.textContent = "Eye break in progress";
    heroCount.textContent = `${status.remainingSecs}s`;
    return;
  }

  if (status.phase === "paused") {
    dot.dataset.state = "paused";
    label.textContent =
      status.pausedForSecs === null
        ? "Paused until you resume"
        : "Reminders paused";
    heroCount.textContent =
      status.pausedForSecs === null ? "Paused" : formatClock(status.pausedForSecs);
    return;
  }

  dot.dataset.state = "working";
  label.textContent = "Next eye break in";
  heroCount.textContent = formatClock(status.nextBreakInSecs);
}

function wireEvents() {
  input("work-minutes").addEventListener("input", () => {
    refreshOutputs();
    scheduleSave();
  });
  input("break-seconds").addEventListener("input", () => {
    refreshOutputs();
    scheduleSave();
  });
  input("snooze-minutes").addEventListener("input", () => {
    refreshOutputs();
    scheduleSave();
  });

  document.querySelectorAll<HTMLButtonElement>("[data-mode]").forEach((btn) => {
    btn.addEventListener("click", () => {
      setMode(btn.dataset.mode as BreakMode);
      scheduleSave();
    });
  });

  ["enabled", "sound", "allow-skip", "strict-mode", "autostart"].forEach((id) => {
    input(id).addEventListener("change", () => {
      syncStrictAvailability();
      scheduleSave();
    });
  });
  input("warn-before").addEventListener("change", () => scheduleSave());

  button("btn-break-now").addEventListener("click", () => void startBreakNow());
  button("pause-15").addEventListener("click", () => void pauseReminders(15));
  button("pause-30").addEventListener("click", () => void pauseReminders(30));
  button("pause-60").addEventListener("click", () => void pauseReminders(60));
  button("pause-inf").addEventListener("click", () => void pauseReminders(null));
  button("resume").addEventListener("click", () => void resumeReminders());

  button("btn-reset").addEventListener("click", () => {
    applySettingsToForm({ ...DEFAULT_SETTINGS });
    scheduleSave();
  });
  button("btn-quit").addEventListener("click", () => void quitApp());
  button("btn-report").addEventListener("click", () => void reportCrashFlow());
  button("btn-logs").addEventListener("click", () => void openLogsFolderFlow());
}

async function init() {
  try {
    const settings = await getSettings();
    applySettingsToForm(settings);
  } catch (error) {
    console.error(error);
    el("save-state").textContent = "Could not load settings";
  }

  wireEvents();
  loaded = true;

  try {
    status = await getStatus();
    renderStatus();
  } catch (error) {
    console.error(error);
  }

  try {
    await onStatus((update) => {
      status = update;
      renderStatus();
    });
  } catch (error) {
    console.error(error);
  }
}

void init().catch((error) => {
  el("save-state").textContent = "Startup error";
  console.error(error);
});
