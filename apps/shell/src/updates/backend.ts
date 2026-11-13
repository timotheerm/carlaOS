// Updating CarlaOS from its GitHub releases. The real backend talks to the
// Rust side. In a plain browser (design work, screenshot tests) a pretend one
// stands in: an update is waiting, and installing it only plays the progress.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { inTauri } from "../native";

export type Available = { version: string; page: string; size: number };
export type Updates = {
  current: string;
  available: Available | null;
  // The desktop host installed on this same machine, if any.
  host_here: { current: string; available: Available | null } | null;
};
export type Progress = { received: number; total: number };
export type HostUpdate = { updating: boolean; version: string };

type Backend = {
  check(): Promise<Updates>;
  // Resolves false when there was nothing to install. When the interface
  // itself is updated it closes before this resolves.
  install(onProgress: (p: Progress) => void): Promise<boolean>;
  updateHost(id: string): Promise<HostUpdate>;
};

const tauri: Backend = {
  check: () => invoke("update_check"),
  async install(onProgress) {
    const stop = await listen<Progress>("update-progress", (e) => onProgress(e.payload));
    try {
      return await invoke<boolean>("update_install");
    } finally {
      stop();
    }
  },
  updateHost: (id) => invoke("hosts_update", { id }),
};

const pause = (ms: number) => new Promise((r) => setTimeout(r, ms));

function pretend(): Backend {
  const next = { version: "0.6.0", page: "", size: 6_400_000 };
  let done = false;
  return {
    async check() {
      await pause(400);
      return done
        ? { current: "0.6.0", available: null, host_here: { current: "0.6.0", available: null } }
        : { current: "0.5.0", available: next, host_here: { current: "0.5.0", available: next } };
    },
    async install(onProgress) {
      const total = next.size * 2;
      for (let step = 1; step <= 10; step++) {
        await pause(150);
        onProgress({ received: (total * step) / 10, total });
      }
      done = true;
      return true;
    },
    async updateHost() {
      await pause(600);
      return { updating: true, version: "0.6.0" };
    },
  };
}

export const updates: Backend = inTauri ? tauri : pretend();

/** "0.10.0" is newer than "0.9.0"; anything unreadable is oldest. */
export function older(a: string, b: string): boolean {
  const parts = (v: string) => {
    const n = v.replace(/^v/, "").split(".").map(Number);
    return n.length === 3 && n.every(Number.isInteger) ? n : [0, 0, 0];
  };
  const [x, y] = [parts(a), parts(b)];
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] < y[i];
  return false;
}
