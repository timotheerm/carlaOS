// Thin wrapper around the Rust side. When the UI runs in a plain browser
// (design work, screenshot tests) the calls fall back to harmless defaults.
import { invoke } from "@tauri-apps/api/core";

export type AppInfo = {
  role: "shell" | "companion";
  version: string;
  api_version: number;
  os: string;
  arch: string;
};

export const inTauri = "__TAURI_INTERNALS__" in window;

export async function appInfo(): Promise<AppInfo> {
  if (!inTauri) {
    return { role: "shell", version: "dev", api_version: 1, os: "browser", arch: "" };
  }
  return invoke<AppInfo>("app_info");
}

export async function quit(): Promise<void> {
  if (inTauri) await invoke("quit");
}

export type BootStatus = { enabled: boolean; stood_down: boolean };

// Whether CarlaOS starts with the Ally. The browser preview pretends.
let previewBoot = false;
export const boot = {
  /** Tell the Rust side the library is on screen, and learn how starting is set up. */
  async ready(): Promise<BootStatus> {
    if (!inTauri) return { enabled: previewBoot, stood_down: false };
    return invoke<BootStatus>("boot_ready");
  },
  /** Administrator only. Resolves with what is in effect afterwards. */
  async set(enabled: boolean): Promise<boolean> {
    if (!inTauri) return (previewBoot = enabled);
    return invoke<boolean>("boot_set", { enabled });
  },
};

export type ReportInfo = { name: string; kind: "panic" | "unexpected" | "hang"; at: number; size: number; new: boolean };

// Crash reports kept by the Rust side. The browser preview has none.
export const crash = {
  async reports(): Promise<ReportInfo[]> {
    if (!inTauri) return [];
    return invoke<ReportInfo[]>("crash_reports");
  },
  async text(name: string): Promise<string> {
    if (!inTauri) return "";
    return invoke<string>("crash_report_text", { name });
  },
  /** The reports have been announced; they stop counting as new. */
  async seen(): Promise<void> {
    if (inTauri) await invoke("crash_seen");
  },
  /** The interface is alive. Sent every few seconds so a freeze can be reported. Never throws. */
  async alive(hidden: boolean): Promise<void> {
    try {
      if (inTauri) await invoke("crash_alive", { hidden });
    } catch {
      /* nothing more can be done */
    }
  },
  /** One line for the trail. Never throws. */
  async note(text: string): Promise<void> {
    try {
      if (inTauri) await invoke("crash_note", { text });
    } catch {
      /* nothing more can be done */
    }
  },
  async openFolder(): Promise<void> {
    if (inTauri) await invoke("crash_open_folder");
  },
};

// The activity log kept by the Rust side: the connection log and the crash trail by time.
// The browser preview has a few made-up lines.
export const activity = {
  async tail(count: number): Promise<string[]> {
    if (!inTauri) return ["2026-11-17 12:04:09.123 UTC link local GET /v1/status answered 200 in 2000 ms", "2026-11-17 12:04:21.456 UTC desktop home: reachable -> unreachable (was reachable for 12333 ms)"];
    return invoke<string[]>("log_tail", { count });
  },
  async text(): Promise<string> {
    if (!inTauri) return (await activity.tail(200)).join("\n") + "\n";
    return invoke<string>("log_text");
  },
  async openFolder(): Promise<void> {
    if (inTauri) await invoke("log_open_folder");
  },
};
