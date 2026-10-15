import "./overlay.css";
import {
  getStatus,
  onBreakEnded,
  onStatus,
  skipBreak,
  snoozeBreak,
  type Status,
} from "./lib";

const HOLD_MS = 3000;

let status: Status | null = null;
let finished = false;
let audioContext: AudioContext | null = null;
let holdFrame: number | undefined;

function must<T extends Element>(selector: string): T {
  const found = document.querySelector<T>(selector);
  if (!found) throw new Error(`Missing element ${selector}`);
  return found;
}

const bar = must<HTMLDivElement>("#progress");
const count = must<HTMLSpanElement>("#count");
const skip = must<HTMLButtonElement>("#skip");
const snooze = must<HTMLButtonElement>("#snooze");
const snoozeLabel = must<HTMLSpanElement>("#snooze-label");


function playTone(frequency: number, delay: number, duration: number) {
  try {
    audioContext ??= new AudioContext();
    if (audioContext.state === "suspended") {
      void audioContext.resume();
    }
    const start = audioContext.currentTime + delay;
    const oscillator = audioContext.createOscillator();
    const gain = audioContext.createGain();
    oscillator.type = "sine";
    oscillator.frequency.value = frequency;
    gain.gain.setValueAtTime(0.0001, start);
    gain.gain.linearRampToValueAtTime(0.07, start + 0.03);
    gain.gain.exponentialRampToValueAtTime(0.0001, start + duration);
    oscillator.connect(gain);
    gain.connect(audioContext.destination);
    oscillator.start(start);
    oscillator.stop(start + duration + 0.05);
  } catch {
    // Sound is a nicety; never let it break the overlay.
  }
}

function playStartChime() {
  playTone(660, 0, 0.35);
  playTone(880, 0.18, 0.45);
}

function playDoneChime() {
  playTone(880, 0, 0.3);
  playTone(660, 0.16, 0.4);
}

function render() {
  if (!status || finished) return;

  const total = Math.max(1, status.totalSecs);
  const remaining = Math.max(0, Math.min(total, status.remainingSecs));
  bar.style.transform = `scaleX(${remaining / total})`;
  count.textContent = String(remaining);

  skip.hidden = !status.allowSkip;
  skip.classList.toggle("hold", status.strictMode);
  skip.title = status.strictMode ? "Hold to skip" : "Skip break";
  skip.setAttribute("aria-label", skip.title);
  snooze.hidden = status.phase !== "breaking";
  snoozeLabel.textContent = `Snooze ${status.snoozeMinutes} min`;
}

function markDone() {
  if (finished) return;
  finished = true;
  document.body.classList.add("done");
}

function beginHold() {
  if (holdFrame !== undefined) return;
  const started = performance.now();
  const tick = (now: number) => {
    const progress = Math.min(1, (now - started) / HOLD_MS);
    skip.style.setProperty("--hold", `${progress * 100}%`);
    if (progress >= 1) {
      cancelHold();
      void skipBreak();
    } else {
      holdFrame = requestAnimationFrame(tick);
    }
  };
  holdFrame = requestAnimationFrame(tick);
}

function cancelHold() {
  if (holdFrame !== undefined) {
    cancelAnimationFrame(holdFrame);
    holdFrame = undefined;
  }
  skip.style.setProperty("--hold", "0%");
}

skip.addEventListener("click", (event) => {
  if (status?.strictMode) {
    event.preventDefault();
    return;
  }
  void skipBreak();
});

skip.addEventListener("pointerdown", (event) => {
  if (status?.strictMode) {
    event.preventDefault();
    beginHold();
  }
});
skip.addEventListener("pointerup", cancelHold);
skip.addEventListener("pointercancel", cancelHold);
skip.addEventListener("pointerleave", cancelHold);
skip.addEventListener("keydown", (event) => {
  if (status?.strictMode && (event.key === " " || event.key === "Enter")) {
    beginHold();
  }
});
skip.addEventListener("keyup", cancelHold);

snooze.addEventListener("click", () => void snoozeBreak());

let wakeTimer: number | undefined;
function wake() {
  document.body.classList.add("awake");
  window.clearTimeout(wakeTimer);
  wakeTimer = window.setTimeout(() => document.body.classList.remove("awake"), 2500);
}
document.addEventListener("mousemove", wake);
document.addEventListener("pointerdown", wake);

document.addEventListener("contextmenu", (event) => event.preventDefault());
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    event.preventDefault();
  }
  if (event.key === "Tab") wake();
});

async function init() {
  status = await getStatus();
  render();
  requestAnimationFrame(() => requestAnimationFrame(() => document.body.classList.add("anim-ready")));

  if (status.phase === "breaking" && status.sound) {
    playStartChime();
  }

  await onStatus((update) => {
    status = update;
    render();
  });

  await onBreakEnded((payload) => {
    if (payload.natural) {
      markDone();
      playDoneChime();
    }
  });
}

void init().catch((error) => {
  console.error(error);
});
