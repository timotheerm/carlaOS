// The real backend talks to the Rust side. In a plain browser (design work,
// screenshot tests) a pretend Ally stands in, so every screen can be reached.
import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../native";
import type { Backend, Change, Item, Report } from "./types";

const tauri: Backend = {
  status: () => invoke("admin_status"),
  setPin: (pin) => invoke("admin_set_pin", { pin }),
  unlock: (pin) => invoke("admin_unlock", { pin }),
  lock: () => invoke("admin_lock"),
  scan: () => invoke("cleanup_scan"),
  apply: (ids) => invoke("cleanup_apply", { ids }),
  undo: (ids) => invoke("cleanup_undo", { ids }),
};

const i = (id: string, kind: Item["kind"], tier: Item["tier"], title: string, detail: string, needs_admin = false): Item => ({
  id, kind, tier, title, detail, needs_admin,
});

const UNUSED = "Not used on a handheld that boots into CarlaOS.";
const ASUS = "ASUS hardware control: fans, power profiles, buttons and firmware updates.";
const XBOX = "Xbox and Game Bar: sign-in, saves, the Xbox button and the full screen experience.";

function pretend(): Backend {
  let pin: string | null = null;
  let unlocked = false;
  let items: Item[] = [
    i("app:Microsoft.BingNews", "app", "recommended", "Microsoft News", UNUSED),
    i("app:Microsoft.BingWeather", "app", "recommended", "Weather", UNUSED),
    i("app:Clipchamp.Clipchamp", "app", "recommended", "Clipchamp video editor", UNUSED),
    i("app:Microsoft.OutlookForWindows", "app", "recommended", "Outlook", UNUSED),
    i("app:Microsoft.WindowsCamera", "app", "recommended", "Camera", "The Ally has no camera."),
    i("run:hkcu_run:OneDrive", "startup", "recommended", "OneDrive at start-up",
      "Keeps OneDrive syncing in the background. Off means files in OneDrive folders stop syncing until OneDrive is opened by hand."),
    i("run:hkcu_run:MicrosoftEdgeAutoLaunch", "startup", "recommended", "Edge at start-up",
      "Opens Edge in the background every time Windows starts."),
    i("svc:DiagTrack", "service", "recommended", "Usage data collection",
      "Collects and uploads usage data in the background. Nothing on the Ally needs it.", true),
    i("set:widgets", "setting", "recommended", "Turn off Widgets",
      "The news and weather panel keeps several background processes running.", true),
    i("set:edge-background", "setting", "recommended", "Stop Edge running in the background",
      "Edge preloads itself at start-up and stays running after it is closed. Edge and CarlaOS keep working normally.", true),
    i("set:setup-nags", "setting", "recommended", "No \"finish setting up\" screens or tips",
      "Stops the full-screen reminders that can appear before CarlaOS after an update, and tips pop-ups."),
    i("app:SpotifyAB.SpotifyMusic", "app", "optional", "Spotify",
      "Often preinstalled. Remove it unless someone listens to Spotify on the Ally."),
    i("run:hkcu_run:Steam", "startup", "optional", "Steam at start-up",
      "CarlaOS starts it when a game needs it. Off means a faster start-up and more free memory; its games update only while it is open."),
    i("svc:SysMain", "service", "optional", "App preloading (SysMain)",
      "Preloads often-used programs into memory. Off frees memory and disk activity for games; desktop programs may open a little slower.", true),
    i("svc:WSearch", "service", "optional", "File search indexing",
      "Indexes files in the background. Off saves processor and disk work; searching for files in Windows gets slower. CarlaOS does not use it.", true),
    i("prog:McAfee LiveSafe", "app", "manual", "McAfee LiveSafe",
      "Trial antivirus. Windows Security takes over by itself once it is uninstalled. CarlaOS could not reinstall it, so uninstall it from Windows Settings yourself."),
    i("app:B9ECED6F.ArmouryCrateSE", "app", "kept", "B9ECED6F.ArmouryCrateSE", ASUS),
    i("svc:ArmouryCrateService", "service", "kept", "ARMOURY CRATE Service", ASUS),
    i("svc:AsusAppService", "service", "kept", "ASUS App Service", ASUS),
    i("app:Microsoft.GamingApp", "app", "kept", "Microsoft.GamingApp", XBOX),
    i("app:Microsoft.XboxGamingOverlay", "app", "kept", "Microsoft.XboxGamingOverlay", XBOX),
    i("svc:XboxGipSvc", "service", "kept", "Xbox Accessory Management Service", XBOX),
    i("svc:WinDefend", "service", "kept", "Microsoft Defender Antivirus Service", "Windows security."),
    i("run:hklm_run:SecurityHealth", "startup", "kept", "SecurityHealth", "Windows security."),
    i("run:hklm_run:RtkAudUService", "startup", "kept", "RtkAudUService", "Part of a hardware driver."),
  ];
  let changes: (Change & { item: Item })[] = [];
  const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
  const report = (): Report => ({ items, changes: changes.map(({ item: _item, ...c }) => c), warnings: [] });

  return {
    status: async () => ({ pin_set: pin !== null, unlocked }),
    setPin: async (p) => {
      pin = p;
      unlocked = true;
    },
    unlock: async (p) => {
      unlocked = p === pin;
      return { result: unlocked ? "unlocked" : "wrong" };
    },
    lock: async () => {
      unlocked = false;
    },
    scan: async () => {
      await wait(500);
      return report();
    },
    apply: async (ids) => {
      await wait(700);
      const done = items.filter((it) => ids.includes(it.id) && (it.tier === "recommended" || it.tier === "optional"));
      items = items.filter((it) => !done.includes(it));
      changes = [
        ...done.map((item) => ({
          item,
          item_id: item.id,
          title: item.title,
          kind: item.kind,
          applied_at: Math.floor(Date.now() / 1000),
          needs_admin: item.needs_admin,
        })),
        ...changes,
      ];
      return { done: done.map((d) => d.id), failed: [], restart: done.some((d) => d.kind !== "app") };
    },
    undo: async (ids) => {
      await wait(700);
      const back = changes.filter((c) => ids.includes(c.item_id));
      changes = changes.filter((c) => !back.includes(c));
      items = [...back.map((c) => c.item), ...items];
      return { done: back.map((c) => c.item_id), failed: [], restart: back.some((c) => c.kind !== "app") };
    },
  };
}

export const backend: Backend = inTauri ? tauri : pretend();
