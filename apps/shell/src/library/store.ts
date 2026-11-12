// The game library as the interface sees it. In the real shell it mirrors the
// catalogue kept by the Rust side; in a plain browser (design work, screenshot
// tests) it serves the sample games so every screen can still be reached.
import { useSyncExternalStore } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { games as sampleGames, hosts as sampleHosts } from "../data/sample";
import type { Game, Host, Kind, NewGame, Session } from "../data/types";
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
type SessionView = { game_id: string; title: string; phase: Session["phase"] };

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
  s && { gameId: s.game_id, title: s.title, phase: s.phase };

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
    await library.scan().catch(() => undefined);
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
    if (place !== "local") throw "Streaming from a desktop is not set up yet.";
    const game = state.games.find((g) => g.id === gameId);
    if (game) set({ session: { gameId, title: game.title, phase: "running" } });
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
