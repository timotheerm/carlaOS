// State of the clean-up screens. It lives outside React because several pages
// share it and it has to survive moving between them.
import { useSyncExternalStore } from "react";
import { backend } from "./backend";
import type { AdminStatus, Outcome, Report, UnlockResult } from "./types";

export type CleanupState = {
  admin: AdminStatus | null; // null until first asked
  busy: "scanning" | "applying" | "undoing" | null;
  report: Report | null;
  selected: ReadonlySet<string>;
  error: string | null;
  last: { verb: "applied" | "undone"; outcome: Outcome } | null;
};

const EMPTY: CleanupState = { admin: null, busy: null, report: null, selected: new Set(), error: null, last: null };

let state = EMPTY;
const listeners = new Set<() => void>();

function set(patch: Partial<CleanupState>) {
  state = { ...state, ...patch };
  listeners.forEach((l) => l());
}

const message = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : "Something went wrong.");

const selectable = (report: Report) => report.items.filter((i) => i.tier === "recommended" || i.tier === "optional");

async function scan(keepSelection = false) {
  set({ busy: "scanning", error: null });
  try {
    const report = await backend.scan();
    const offered = selectable(report);
    const selected = new Set(
      offered.filter((i) => (keepSelection ? state.selected.has(i.id) : i.tier === "recommended")).map((i) => i.id),
    );
    set({ report, selected, busy: null });
  } catch (e) {
    set({ busy: null, error: message(e) });
  }
}

async function change(verb: "applied" | "undone", ids: string[]) {
  if (ids.length === 0 || state.busy) return;
  set({ busy: verb === "applied" ? "applying" : "undoing", error: null, last: null });
  try {
    const outcome = verb === "applied" ? await backend.apply(ids) : await backend.undo(ids);
    set({ last: { verb, outcome } });
    // Look again, so the lists show the machine as it now is.
    await scan(verb === "undone");
  } catch (e) {
    set({ busy: null, error: message(e) });
  }
}

export const cleanup = {
  async open() {
    set({ admin: await backend.status().catch(() => ({ pin_set: false, unlocked: false })) });
    if (state.admin?.unlocked && !state.report) void scan();
  },
  async setPin(pin: string) {
    await backend.setPin(pin);
    set({ admin: { pin_set: true, unlocked: true } });
    void scan();
  },
  async unlock(pin: string): Promise<UnlockResult> {
    const result = await backend.unlock(pin);
    if (result.result === "unlocked") {
      set({ admin: { pin_set: true, unlocked: true } });
      void scan();
    }
    return result;
  },
  /** Leaving the clean-up pages locks them again and forgets what was shown. */
  leave() {
    if (state === EMPTY) return;
    void backend.lock().catch(() => undefined);
    state = EMPTY;
    listeners.forEach((l) => l());
  },
  scan: () => scan(),
  toggle(id: string) {
    const selected = new Set(state.selected);
    if (!selected.delete(id)) selected.add(id);
    set({ selected });
  },
  select(which: "recommended" | "none") {
    if (!state.report) return;
    const ids = which === "none" ? [] : selectable(state.report).filter((i) => i.tier === "recommended").map((i) => i.id);
    set({ selected: new Set(ids) });
  },
  apply: () => change("applied", [...state.selected]),
  undo: (ids: string[]) => change("undone", ids),
};

export function useCleanup(): CleanupState {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => state,
  );
}
