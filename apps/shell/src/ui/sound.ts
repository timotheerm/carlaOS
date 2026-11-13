// Small interface sounds, made here from plain tones so nothing is shipped or
// fetched: a tick when focus moves, a chime when something opens, a rising
// phrase when a game starts. Quiet on purpose, and one setting turns them off.

export type Cue = "move" | "select" | "back" | "section" | "launch" | "favourite";

const KEY = "carla.sounds";

let on = true;
try {
  on = localStorage.getItem(KEY) !== "off";
} catch {
  // no storage: keep the default
}

let context: AudioContext | null = null;
let lastMove = 0;

export const soundsOn = () => on;

export function setSounds(next: boolean) {
  on = next;
  try {
    localStorage.setItem(KEY, next ? "on" : "off");
  } catch {
    // not remembered, still applied
  }
  if (next) play("select");
}

// One note: a sine that fades out, starting `after` seconds from now.
function note(ctx: AudioContext, hz: number, after: number, length: number, level: number, slideTo?: number) {
  const start = ctx.currentTime + after;
  const tone = ctx.createOscillator();
  const gain = ctx.createGain();
  tone.type = "sine";
  tone.frequency.setValueAtTime(hz, start);
  if (slideTo) tone.frequency.exponentialRampToValueAtTime(slideTo, start + length);
  gain.gain.setValueAtTime(0.0001, start);
  gain.gain.exponentialRampToValueAtTime(level, start + 0.012);
  gain.gain.exponentialRampToValueAtTime(0.0001, start + length);
  tone.connect(gain).connect(ctx.destination);
  tone.start(start);
  tone.stop(start + length + 0.02);
}

export function play(cue: Cue) {
  if (!on || document.hidden) return;
  try {
    context ??= new AudioContext();
    const ctx = context;
    // The browser keeps sound off until the first press; ask each time.
    if (ctx.state === "suspended") void ctx.resume().catch(() => undefined);
    if (ctx.state !== "running") return;
    if (cue === "move") {
      // A held direction repeats quickly; not every step needs a tick.
      const now = performance.now();
      if (now - lastMove < 60) return;
      lastMove = now;
      note(ctx, 1320, 0, 0.05, 0.035, 990);
    } else if (cue === "select") {
      note(ctx, 784, 0, 0.09, 0.06);
      note(ctx, 1175, 0.06, 0.14, 0.06);
    } else if (cue === "back") {
      note(ctx, 659, 0, 0.12, 0.05, 440);
    } else if (cue === "section") {
      note(ctx, 880, 0, 0.1, 0.045, 1100);
    } else if (cue === "favourite") {
      note(ctx, 988, 0, 0.08, 0.05);
      note(ctx, 1319, 0.07, 0.08, 0.05);
      note(ctx, 1568, 0.14, 0.18, 0.05);
    } else {
      [523, 659, 784, 1047].forEach((hz, i) => note(ctx, hz, i * 0.09, 0.32, 0.06));
    }
  } catch {
    // No sound on this machine: the interface works the same without.
  }
}

/** Tick when focus moves from one thing to another, chime when one is pressed. */
export function installSounds(): () => void {
  const onFocus = (e: FocusEvent) => {
    const from = e.relatedTarget as HTMLElement | null;
    const to = e.target as HTMLElement | null;
    // Only real moves: not the focus a new page puts somewhere by itself.
    if (from?.matches?.("[data-focus]") && from.isConnected && to?.matches?.("[data-focus]")) play("move");
  };
  const onClick = (e: MouseEvent) => {
    const el = (e.target as HTMLElement | null)?.closest?.("[data-focus]");
    if (el && !el.hasAttribute("data-quiet")) play("select");
  };
  document.addEventListener("focusin", onFocus);
  document.addEventListener("click", onClick);
  return () => {
    document.removeEventListener("focusin", onFocus);
    document.removeEventListener("click", onClick);
  };
}
