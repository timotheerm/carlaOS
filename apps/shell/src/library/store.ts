// The game library as the interface sees it. In the real shell it mirrors the
// catalogue kept by the Rust side; in a plain browser (design work, screenshot
// tests) it serves the sample games so every screen can still be reached.
import { useSyncExternalStore } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { games as sampleGames, hosts as sampleHosts } from "../data/sample";
import type { FoundHost, Game, Host, HostState, Kind, NewGame, Session } from "../data/types";
import { inTauri } from "../native";

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
};
type Linked = { host: HostView; games: number; warning: string | null };
type Found = { address: string; hello: { name: string; id: string } };

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

// "today, 21:05", "yesterday, 22:14", "3 March".
function when(seconds: number | null): string {
  if (seconds === null) return "never";
  const then = new Date(seconds * 1000);
  const days = Math.round((new Date().setHours(0, 0, 0, 0) - new Date(then).setHours(0, 0, 0, 0)) / 86_400_000);
  const time = then.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  if (days <= 0) return `today, ${time}`;
  if (days === 1) return `yesterday, ${time}`;
  return then.toLocaleDateString([], { day: "numeric", month: "long" });
}

const awake = (state: HostState) => state === "ready" || state === "busy" || state === "not_ready";

const fromHost = (h: HostView): Host => ({
  id: h.id,
  name: h.name,
  state: h.state,
  route: awake(h.state) ? "local" : null,
  latencyMs: h.latency_ms,
  hardware: h.hardware,
  lastSeen: awake(h.state) ? "now" : when(h.last_seen),
  playing: h.playing,
  problem: h.problem,
  address: h.address,
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
    if (place !== "local" && host?.state !== "ready" && host?.state !== "busy") {
      throw `${host?.name ?? "That desktop"} did not answer. It may be off or asleep.`;
    }
    const game = state.games.find((g) => g.id === gameId);
    if (game) set({ session: { gameId, title: game.title, phase: "running", stream: place !== "local" } });
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
      description: "",
      platform: "Standalone",
      kind: game.kind,
      hue: hue(game.title),
      places: ["local"],
      playtimeMinutes: 0,
      added: now,
      favourite: false,
      manual: true,
    };
    set({ games: [...state.games.filter((g) => g.id !== added.id), added] });
  },

  async removeManual(id: string) {
    if (inTauri) {
      await invoke("library_remove_manual", { id });
      await reload();
    } else set({ games: state.games.filter((g) => g.id !== id) });
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
