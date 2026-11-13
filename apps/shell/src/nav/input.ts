// Turns keyboard and gamepad input into a small set of actions.
//
// Gamepad polling uses the browser Gamepad API for now. It only sees the
// controller while the shell is focused, which is what we want here; the native
// Rust reader planned for later replaces it without changing the actions.

import { move, type Dir } from "./focus";

export type Action = "back" | "prev" | "next" | "favourite" | "options" | "search";

type Handler = (action: Action) => void;

const KEY_DIR: Record<string, Dir> = {
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
};

const KEY_ACTION: Record<string, Action> = {
  Escape: "back",
  Backspace: "back",
  q: "prev",
  e: "next",
  f: "favourite",
  m: "options",
  "/": "search",
};

// Standard gamepad mapping indices.
const PAD_ACTION: Record<number, Action> = {
  1: "back", // B
  2: "favourite", // X
  3: "search", // Y
  4: "prev", // LB
  5: "next", // RB
  9: "options", // Menu
};
const PAD_DIR: Record<number, Dir> = { 12: "up", 13: "down", 14: "left", 15: "right" };

const FIRST_REPEAT_MS = 380;
const REPEAT_MS = 130;
const STICK_THRESHOLD = 0.55;

// Changing section is a big step, so a held LB or RB repeats slowly. Every
// other button does its thing once per press, however long it is held.
const REPEATS: ReadonlySet<Action> = new Set<Action>(["prev", "next"]);
const ACTION_FIRST_REPEAT_MS = 600;
const ACTION_REPEAT_MS = 400;

// What is held right now. Kept here, not inside installInput, so that it
// survives the listener being installed again: a button that is still down
// must never look like a new press.
let heldDir: Dir | null = null;
let nextRepeat = 0;
const wasDown = new Set<number>();
const actionRepeat = new Map<number, number>(); // button -> time of its next repeat
const keyRepeat = new Map<string, number>(); // key -> time of its next repeat

/** Decide whether a held button acts again at `now`. First call is the press. */
export function heldFires(next: Map<number | string, number>, id: number | string, repeats: boolean, isNew: boolean, now: number): boolean {
  if (isNew) {
    next.set(id, now + ACTION_FIRST_REPEAT_MS);
    return true;
  }
  const due = next.get(id);
  if (!repeats || due === undefined || now < due) return false;
  next.set(id, now + ACTION_REPEAT_MS);
  return true;
}

export function installInput(onAction: Handler): () => void {
  const onKey = (e: KeyboardEvent) => {
    // While typing in a field, only up and down leave it.
    const typing = e.target instanceof HTMLInputElement;
    if (typing && e.key !== "ArrowUp" && e.key !== "ArrowDown" && e.key !== "Escape") return;
    const dir = KEY_DIR[e.key];
    if (dir) {
      e.preventDefault();
      move(dir);
      return;
    }
    const action = KEY_ACTION[e.key];
    if (action) {
      e.preventDefault();
      // The keyboard repeats a held key some thirty times a second.
      if (heldFires(keyRepeat as Map<number | string, number>, e.key, REPEATS.has(action), !e.repeat, performance.now())) onAction(action);
    }
  };
  const onKeyUp = (e: KeyboardEvent) => keyRepeat.delete(e.key);
  window.addEventListener("keydown", onKey);
  window.addEventListener("keyup", onKeyUp);

  let raf = 0;

  const poll = (now: number) => {
    raf = requestAnimationFrame(poll);
    // The same controller is playing the game while one runs in front of us.
    // Forget what was held, so nothing fires by itself on the way back.
    if (!document.hasFocus()) {
      heldDir = null;
      for (let i = 0; i < 16; i++) wasDown.add(i);
      actionRepeat.clear();
      return;
    }
    const pad = navigator.getGamepads?.().find((p) => p && p.connected);
    if (!pad) return;

    let dir: Dir | null = null;
    for (const [index, d] of Object.entries(PAD_DIR)) {
      if (pad.buttons[Number(index)]?.pressed) dir = d;
    }
    if (!dir) {
      const [x = 0, y = 0] = pad.axes;
      if (Math.abs(x) > Math.abs(y)) {
        if (x > STICK_THRESHOLD) dir = "right";
        else if (x < -STICK_THRESHOLD) dir = "left";
      } else if (y > STICK_THRESHOLD) dir = "down";
      else if (y < -STICK_THRESHOLD) dir = "up";
    }
    if (dir !== heldDir) {
      heldDir = dir;
      if (dir) {
        move(dir);
        nextRepeat = now + FIRST_REPEAT_MS;
      }
    } else if (dir && now >= nextRepeat) {
      move(dir);
      nextRepeat = now + REPEAT_MS;
    }

    // A acts on the focused element, exactly like a tap.
    const press = (index: number, fire: () => void) => {
      const down = !!pad.buttons[index]?.pressed;
      if (down && !wasDown.has(index)) fire();
      if (down) wasDown.add(index);
      else wasDown.delete(index);
    };
    press(0, () => (document.activeElement as HTMLElement | null)?.click());
    for (const [index, action] of Object.entries(PAD_ACTION)) {
      const button = Number(index);
      const down = !!pad.buttons[button]?.pressed;
      if (down) {
        if (heldFires(actionRepeat as Map<number | string, number>, button, REPEATS.has(action), !wasDown.has(button), now)) onAction(action);
        wasDown.add(button);
      } else {
        wasDown.delete(button);
        actionRepeat.delete(button);
      }
    }
  };

  // No polling while hidden, so the shell costs nothing behind a game.
  const onVisibility = () => {
    cancelAnimationFrame(raf);
    if (!document.hidden) raf = requestAnimationFrame(poll);
  };
  document.addEventListener("visibilitychange", onVisibility);
  onVisibility();

  return () => {
    window.removeEventListener("keydown", onKey);
    window.removeEventListener("keyup", onKeyUp);
    document.removeEventListener("visibilitychange", onVisibility);
    cancelAnimationFrame(raf);
  };
}
