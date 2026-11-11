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

const inTauri = "__TAURI_INTERNALS__" in window;

export async function appInfo(): Promise<AppInfo> {
  if (!inTauri) {
    return { role: "shell", version: "dev", api_version: 1, os: "browser", arch: "" };
  }
  return invoke<AppInfo>("app_info");
}

export async function quit(): Promise<void> {
  if (inTauri) await invoke("quit");
}
