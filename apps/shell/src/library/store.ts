// The game library as the interface sees it. In the real shell it mirrors the
// catalogue kept by the Rust side; in a plain browser (design work, screenshot
// tests) it serves the sample games so every screen can still be reached.
import { useSyncExternalStore } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { games as sampleGames, hosts as sampleHosts } from "../data/sample";
import { sampleDetails, sampleMatches } from "../data/sample";
import type { Details, FoundHost, Game, Host, HostState, Kind, Match, NewGame, Saves, Session } from "../data/types";
import { inTauri } from "../native";
import { when } from "../ui/parts";

type State = { games: Game[]; hosts: Host[]; session: Session | null; scanning: boolean };

// What the Rust side sends for one game.
type GameView = {
  id: string;
  title: string;
  description: string;
  platform: string;
  kind: Kind;
  places: string[];
  playtime_minutes: number;
  last_played: number | null;
  added: number;
  favourite: boolean;
  cover: string | null;
  hero: string | null;
  manual: boolean;
};
type SessionView = { game_id: string; title: string; phase: Session["phase"]; stream: boolean };
// What the Rust side sends for one linked desktop.
type HostView = {
  id: string;
  name: string;
  state: HostState;
  playing: string | null;
  problem: string | null;
  latency_ms: number | null;
  hardware: string;
  last_seen: number | null;
  address: string;
  route: "local" | "remote" | null;
  can_wake: boolean;
  version?: string | null;
};
type Linked = { host: HostView; games: number; warning: string | null };
type Found = { address: string; hello: { name: string; id: string } };

type DetailsView = Omit<Details, "achievementsFrom"> & { achievements_from: string | null };
type Fetched = { name: string; description: string; cover: string | null };

// Saves as the browser preview pretends them, so backing up can be tried.
const previewSaves = new Map<string, Saves | null>();
function preview(gameId: string): Saves | null {
  if (!previewSaves.has(gameId)) {
    const game = state.games.find((g) => g.id === gameId);
    previewSaves.set(gameId, game ? sampleDetails(game).saves : null);
  }
  return previewSaves.get(gameId) ?? null;
}

const FAV_KEY = "carla.favourites";
const iso = (seconds: number) => new Date(seconds * 1000).toISOString();

// A steady colour per title for games without artwork.
function hue(title: string): number {
  let h = 0;
  for (const c of title) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

const fromView = (g: GameView): Game => ({
  id: g.id,
  title: g.title,
  description: g.description,
  platform: g.platform,
  kind: g.kind,
  hue: hue(g.title),
  places: g.places,
  playtimeMinutes: g.playtime_minutes,
  lastPlayed: g.last_played === null ? undefined : iso(g.last_played),
  added: iso(g.added),
  favourite: g.favourite,
  cover: g.cover ? convertFileSrc(g.cover) : undefined,
  hero: g.hero ? convertFileSrc(g.hero) : undefined,
  manual: g.manual,
});

const fromSession = (s: SessionView | null): Session | null =>
  s && { gameId: s.game_id, title: s.title, phase: s.phase, stream: s.stream };

const awake = (state: HostState) => state === "ready" || state === "busy" || state === "not_ready";

const fromHost = (h: HostView): Host => ({
  id: h.id,
  name: h.name,
  state: h.state,
  route: h.route,
  latencyMs: h.latency_ms,
  hardware: h.hardware,
  lastSeen: awake(h.state) ? "now" : when(h.last_seen),
  playing: h.playing,
  problem: h.problem,
  address: h.address,
  canWake: h.can_wake,
  version: h.version ?? null,
});

// How often desktops are asked how they are while the library is on screen.
const HOSTS_EVERY = 30_000;

function sample(): Game[] {
  let saved: string[] = ["petal-kart", "starfall-odyssey", "moss-and-ember"];
  try {
    saved = JSON.parse(localStorage.getItem(FAV_KEY) ?? "null") ?? saved;
  } catch {
    // keep the defaults
  }
  return sampleGames.map((g) => ({ ...g, favourite: saved.includes(g.id) }));
}

let state: State = inTauri
  ? { games: [], hosts: [], session: null, scanning: false }
  : { games: sample(), hosts: sampleHosts, session: null, scanning: false };
const listeners = new Set<() => void>();

function set(patch: Partial<State>) {
  state = { ...state, ...patch };
  listeners.forEach((l) => l());
}

export const message = (e: unknown) =>
  typeof e === "string" ? e : e instanceof Error ? e.message : "Something went wrong.";

async function reload() {
  set({ games: (await invoke<GameView[]>("library_list")).map(fromView) });
}

let started = false;
let askingHosts = false;

/** Ask every desktop how it is. Their games can change with the answer. */
async function refreshHosts() {
  if (!inTauri || askingHosts) return;
  askingHosts = true;
  try {
    set({ hosts: (await invoke<HostView[]>("hosts_refresh")).map(fromHost) });
    await reload();
  } finally {
    askingHosts = false;
  }
}

export const library = {
  /** Called once when the shell starts. */
  async start() {
    if (started || !inTauri) return;
    started = true;
    await listen<SessionView | null>("library://session", (e) => {
      set({ session: fromSession(e.payload) });
      // Playtime and "recently played" change when a game ends.
      if (e.payload === null) void reload().catch(() => undefined);
    });
    set({ session: fromSession(await invoke<SessionView | null>("library_session")) });
    await reload().catch(() => undefined); // show what we knew last time at once
    set({ hosts: (await invoke<HostView[]>("hosts_list").catch(() => [])).map(fromHost) });
    void refreshHosts().catch(() => undefined);
    // Nothing is asked while a game is in front or the screen is off.
    setInterval(() => {
      if (state.session === null && state.hosts.length > 0 && document.visibilityState === "visible" && document.hasFocus()) {
        void refreshHosts().catch(() => undefined);
      }
    }, HOSTS_EVERY);
    await library.scan().catch(() => undefined);
  },

  refreshHosts,

  /** Wake a sleeping desktop, then keep asking until it answers. */
  async wake(id: string) {
    if (inTauri) {
      await invoke("hosts_wake", { id });
      // Waking takes from a few seconds to a minute.
      for (const after of [8_000, 16_000, 28_000, 45_000, 70_000]) {
        setTimeout(() => {
          const host = state.hosts.find((h) => h.id === id);
          if (host && host.state !== "ready" && host.state !== "busy") void refreshHosts().catch(() => undefined);
        }, after);
      }
      return;
    }
    setTimeout(() => {
      set({ hosts: state.hosts.map((h) => (h.id === id ? { ...h, state: "ready", route: "local", latencyMs: 6, lastSeen: "now" } : h)) });
    }, 1500);
  },

  /** Look for desktops running the companion on this network. */
  async discover(): Promise<FoundHost[]> {
    if (!inTauri) {
      await new Promise((r) => setTimeout(r, 400));
      return [{ address: "192.168.1.20", name: "HOME PC", id: "home" }].filter((f) => !state.hosts.some((h) => h.id === f.id && h.address));
    }
    return (await invoke<Found[]>("hosts_discover")).map((f) => ({ address: f.address, name: f.hello.name, id: f.hello.id }));
  },

  /** Link a desktop with the code it shows. Resolves with a warning, if any. */
  async link(address: string, code: string): Promise<{ name: string; games: number; warning: string | null }> {
    if (!inTauri) {
      await new Promise((r) => setTimeout(r, 400));
      if (code === "000000") throw "That is not the code on the desktop.";
      const host: Host = { ...sampleHosts[0], id: "home", address, state: "ready", lastSeen: "now" };
      set({ hosts: [...state.hosts.filter((h) => h.id !== host.id), host] });
      return { name: host.name, games: state.games.filter((g) => g.places.includes(host.id)).length, warning: null };
    }
    const linked = await invoke<Linked>("hosts_link", { address, code });
    const host = fromHost(linked.host);
    set({ hosts: [...state.hosts.filter((h) => h.id !== host.id), host] });
    await reload();
    return { name: host.name, games: linked.games, warning: linked.warning };
  },

  async removeHost(id: string) {
    if (inTauri) {
      await invoke("hosts_remove", { id });
      await reload();
    }
    set({ hosts: state.hosts.filter((h) => h.id !== id) });
  },

  /** Look through one desktop's games again now. Resolves with how many it has. */
  async rescanHost(id: string): Promise<number> {
    if (!inTauri) {
      await new Promise((r) => setTimeout(r, 400));
      return state.games.filter((g) => g.places.includes(id)).length;
    }
    const games = await invoke<number>("hosts_rescan", { id });
    await reload();
    return games;
  },

  /** Send a desktop to sleep. */
  async sleepHost(id: string) {
    if (inTauri) {
      await invoke("hosts_sleep", { id });
      setTimeout(() => void refreshHosts().catch(() => undefined), 8_000);
      return;
    }
    setTimeout(() => {
      set({ hosts: state.hosts.map((h) => (h.id === id ? { ...h, state: "asleep", route: null, latencyMs: null, lastSeen: "today" } : h)) });
    }, 800);
  },

  /** Look again at what is installed. */
  async scan() {
    if (!inTauri) return;
    set({ scanning: true });
    try {
      set({ games: (await invoke<GameView[]>("library_scan")).map(fromView) });
    } finally {
      set({ scanning: false });
    }
  },

  async setFavourite(id: string, favourite: boolean) {
    set({ games: state.games.map((g) => (g.id === id ? { ...g, favourite } : g)) });
    if (inTauri) await invoke("library_set_favourite", { id, favourite });
    else localStorage.setItem(FAV_KEY, JSON.stringify(state.games.filter((g) => g.favourite).map((g) => g.id)));
  },

  /** Start a game. Rejects with a sentence that can be shown as it is. */
  async launch(gameId: string, place: string) {
    if (inTauri) {
      set({ session: fromSession(await invoke<SessionView>("library_launch", { gameId, place })) });
      return;
    }
    const host = state.hosts.find((h) => h.id === place);
    const game = state.games.find((g) => g.id === gameId);
    if (!game) return;
    if (place !== "local" && host?.state !== "ready" && host?.state !== "busy") {
      if (!host?.canWake) throw `${host?.name ?? "That desktop"} did not answer. It may be off or asleep.`;
      set({ session: { gameId, title: game.title, phase: "waking", stream: true } });
      await new Promise((r) => setTimeout(r, 1500));
      if (state.session === null) return; // the player gave up
      set({ hosts: state.hosts.map((h) => (h.id === place ? { ...h, state: "ready", route: "local", lastSeen: "now" } : h)) });
    }
    set({ session: { gameId, title: game.title, phase: "running", stream: place !== "local" } });
  },

  /** The player says the game is over. */
  async endSession() {
    if (inTauri) await invoke("library_end_session");
    else set({ session: null });
  },

  pick: (what: "program" | "image") =>
    inTauri ? invoke<string | null>("library_pick", { what }) : Promise.resolve(what === "program" ? "C:\\Games\\Example\\example.exe" : null),

  async addManual(game: NewGame) {
    if (inTauri) {
      await invoke<string>("library_add_manual", { game });
      await reload();
      return;
    }
    const now = new Date().toISOString();
    const added: Game = {
      id: `manual:${game.exe.toLowerCase()}`,
      title: game.title,
      description: game.description,
      platform: "Standalone",
      kind: game.kind,
      hue: hue(game.title),
      places: ["local"],
      playtimeMinutes: 0,
      added: now,
      favourite: false,
      manual: true,
      cover: game.cover ?? undefined,
    };
    set({ games: [...state.games.filter((g) => g.id !== added.id), added] });
  },

  async removeManual(id: string) {
    if (inTauri) {
      await invoke("library_remove_manual", { id });
      await reload();
    } else set({ games: state.games.filter((g) => g.id !== id) });
  },

  /** Achievements, milestones and saves of one game. */
  async details(gameId: string): Promise<Details> {
    if (inTauri) {
      const { achievements_from, ...rest } = await invoke<DetailsView>("library_details", { gameId });
      return { ...rest, achievementsFrom: achievements_from };
    }
    const game = state.games.find((g) => g.id === gameId);
    if (!game) return { achievements: [], achievementsFrom: null, milestones: [], saves: null, sessions: 0 };
    return { ...sampleDetails(game), saves: preview(gameId) };
  },

  /** Copy the saves aside now. Resolves with the new overview. */
  async backupSaves(gameId: string): Promise<Saves> {
    if (inTauri) return invoke<Saves>("saves_backup", { gameId });
    const saves = preview(gameId);
    if (!saves) throw "CarlaOS does not know where this game keeps its saves.";
    const at = Math.floor(Date.now() / 1000);
    const next = { ...saves, backups: [{ id: String(at), at, files: saves.files, bytes: saves.bytes }, ...saves.backups].slice(0, 5) };
    previewSaves.set(gameId, next);
    return next;
  },

  /** Put a backup back. The saves of now are backed up first. Administrator only. */
  async restoreSaves(gameId: string, backupId: string): Promise<Saves> {
    if (inTauri) return invoke<Saves>("saves_restore", { gameId, backupId });
    const saves = preview(gameId);
    if (!saves) throw "CarlaOS does not know where this game keeps its saves.";
    const wanted = saves.backups.find((b) => b.id === backupId);
    const at = Math.floor(Date.now() / 1000);
    const next = { ...saves, modified: wanted?.at ?? saves.modified, backups: [{ id: String(at), at, files: saves.files, bytes: saves.bytes }, ...saves.backups].slice(0, 5) };
    previewSaves.set(gameId, next);
    return next;
  },

  /** The administrator says where a game keeps its saves. Null when nothing was chosen. */
  async pickSaveFolder(gameId: string): Promise<Saves | null> {
    if (inTauri) return invoke<Saves | null>("saves_pick_folder", { gameId });
    const next: Saves = { folder: "C:\\Users\\Carla\\Saved Games\\Example", files: 3, bytes: 1_400_000, modified: Math.floor(Date.now() / 1000) - 7200, backups: [] };
    previewSaves.set(gameId, next);
    return next;
  },

  async forgetSaveFolder(gameId: string) {
    if (inTauri) await invoke("saves_forget_folder", { gameId });
    else previewSaves.set(gameId, null);
  },

  /** Look a game up in the public store by name. Administrator only. */
  async findOnline(term: string): Promise<Match[]> {
    if (inTauri) return invoke<Match[]>("meta_search", { term });
    await new Promise((r) => setTimeout(r, 300));
    return sampleMatches(term);
  },

  /** Fetch the description and cover of one match. The cover is a file path. */
  async fetchOnline(id: string): Promise<{ name: string; description: string; cover: string | null; preview: string | null }> {
    if (inTauri) {
      const got = await invoke<Fetched>("meta_fetch", { id });
      return { ...got, preview: got.cover ? convertFileSrc(got.cover) : null };
    }
    await new Promise((r) => setTimeout(r, 300));
    const name = sampleMatches("").find((m) => m.id === id)?.name ?? "Example";
    return { name, description: "A short hand-drawn puzzle game about lighting the way home.", cover: null, preview: null };
  },
};

export function useLibrary(): State {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => state,
  );
}
