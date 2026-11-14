// The game library as the interface sees it. In the real shell it mirrors the
// catalogue kept by the Rust side; in a plain browser (design work, screenshot
// tests) it serves the sample games so every screen can still be reached.
import { useSyncExternalStore } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { games as sampleGames, hosts as sampleHosts } from "../data/sample";
import { sampleArt, sampleDetails, sampleMatches } from "../data/sample";
import type { ArtChoice, ArtSearch, Console, ConsoleHint, Details, Inspected, FoundHost, Game, Host, HostPicture, HostState, Kind, Match, NewGame, SaveSync, Saves, Session } from "../data/types";
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
  origin: string;
  own_art: boolean;
};
type SessionView = { game_id: string; title: string; phase: Session["phase"]; stream: boolean; cloud: boolean };
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

// Desktops allowed to administer the Ally, as the browser preview pretends.
const previewAdmin = new Set<string>();

// The SteamGridDB key as the browser preview pretends it: kept or not.
let previewKey = false;

// "C:\\Games\\Paper Lanterns\\lanterns.exe" suggests "Paper Lanterns".
function previewTitle(exe: string): string {
  const parts = exe.split(/[\\/]/).filter(Boolean);
  const file = (parts.pop() ?? "").replace(/\.[^.]+$/, "");
  const folder = parts.pop() ?? "";
  return (/^(game|launcher|start|play)$/i.test(file) || file.length < 3 ? folder || file : file).replace(/[_-]+/g, " ").trim();
}

// Consoles as the browser preview pretends them.
let previewConsoles: Console[] = [
  { id: "c0", name: "PlayStation 2", emulator: "C:\\Emulators\\PCSX2\\pcsx2-qt.exe", options: '-batch -fullscreen -- "{game}"', folder: "D:\\My Games\\PS2", endings: ["chd", "iso"] },
];

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
  origin: g.origin || undefined,
  ownArt: g.own_art,
});

const fromSession = (s: SessionView | null): Session | null =>
  s && { gameId: s.game_id, title: s.title, phase: s.phase, stream: s.stream, cloud: s.cloud };

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

const previewPictures = new Map<string, string>();
function previewPicture(id: string): HostPicture {
  return {
    setting: previewPictures.get(id) ?? "auto",
    settings: ["auto", "auto120", "smooth", "sharp", "balanced", "light", "lightest"],
    // One sample desktop shows what a measured stream looks like.
    last: id === "home" ? { level: "sharp", automatic: true, height: 1080, fps: 60, mbps: 25, latency_ms: 4, link_kbps: 310_000, away: false, at: Math.floor(Date.now() / 1000) - 5400 } : null,
  };
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
    // The administrator changed the library from a desktop.
    await listen("library://changed", () => void reload().catch(() => undefined));
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

  /** Whether a desktop may administer this Ally. */
  async hostAdmin(id: string): Promise<boolean> {
    if (inTauri) return invoke<boolean>("hosts_admin", { id });
    return previewAdmin.has(id);
  },

  /** Allow a desktop to administer this Ally, or take that back. Administrator only. */
  async setHostAdmin(id: string, allowed: boolean) {
    if (inTauri) await invoke("hosts_set_admin", { id, allowed });
    else if (allowed) previewAdmin.add(id);
    else previewAdmin.delete(id);
  },

  /** A desktop's picture setting and what its last stream got. */
  async hostPicture(id: string): Promise<HostPicture> {
    if (inTauri) return invoke<HostPicture>("hosts_picture", { id });
    return previewPicture(id);
  },

  /** Choose the picture of streams from a desktop. Administrator only. */
  async setHostPicture(id: string, setting: string): Promise<HostPicture> {
    if (inTauri) return invoke<HostPicture>("hosts_set_picture", { id, setting });
    previewPictures.set(id, setting);
    return previewPicture(id);
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
    if (place === "cloud") {
      set({ session: { gameId, title: game.title, phase: "starting", stream: false, cloud: true } });
      await new Promise((r) => setTimeout(r, 1500));
      if (state.session !== null) set({ session: { gameId, title: game.title, phase: "running", stream: false, cloud: true } });
      return;
    }
    if (place !== "local" && host?.state !== "ready" && host?.state !== "busy") {
      if (!host?.canWake) throw `${host?.name ?? "That desktop"} did not answer. It may be off or asleep.`;
      set({ session: { gameId, title: game.title, phase: "waking", stream: true, cloud: false } });
      await new Promise((r) => setTimeout(r, 1500));
      if (state.session === null) return; // the player gave up
      set({ hosts: state.hosts.map((h) => (h.id === place ? { ...h, state: "ready", route: "local", lastSeen: "now" } : h)) });
    }
    set({ session: { gameId, title: game.title, phase: "running", stream: place !== "local", cloud: false } });
  },

  /** Open a cloud gaming service itself: to browse it, or to sign in once. */
  async openCloud(service: string, name: string) {
    if (inTauri) {
      set({ session: fromSession(await invoke<SessionView>("cloud_open", { service })) });
      return;
    }
    set({ session: { gameId: `cloudservice:${service}`, title: name, phase: "running", stream: false, cloud: true } });
  },

  /** Which service a link is for, and the name it suggests. Administrator only. */
  async inspectCloud(link: string): Promise<{ service: string; title: string }> {
    if (inTauri) return invoke<{ service: string; title: string }>("cloud_inspect", { link });
    const known: [RegExp, string][] = [
      [/^https:\/\/play\.geforcenow\.com\//, "GeForce NOW"],
      [/^https:\/\/www\.xbox\.com\/([a-z]{2}(-[a-z]{2})?\/)?play/i, "Xbox Cloud Gaming"],
      [/^https:\/\/cloud\.boosteroid\.com\//, "Boosteroid"],
      [/^https:\/\/luna\.amazon\.com\//, "Amazon Luna"],
    ];
    const found = known.find(([pattern]) => pattern.test(link.trim()));
    if (!found) throw "That is not a link to a game on GeForce NOW, Xbox Cloud Gaming, Boosteroid or Luna. Copy the game's link from its page there.";
    const slug = /\/(?:games|launch)\/([a-z0-9-]+)/i.exec(link)?.[1] ?? "";
    return { service: found[1], title: slug.split("-").filter(Boolean).map((w) => w[0].toUpperCase() + w.slice(1)).join(" ") };
  },

  /** Add a cloud game. Resolves with its id. Administrator only. */
  async addCloud(link: string, title: string): Promise<string> {
    if (inTauri) {
      const id = await invoke<string>("cloud_add", { link, title });
      await reload();
      return id;
    }
    const { service } = await library.inspectCloud(link);
    const id = `cloud:${link.length}${title.length}`;
    const added: Game = { id, title, description: "", platform: service, kind: "game", hue: hue(title), places: ["cloud"], playtimeMinutes: 0, added: new Date().toISOString(), favourite: false, manual: true };
    set({ games: [...state.games.filter((g) => g.id !== id), added] });
    return id;
  },

  /** The player says the game is over. */
  async endSession() {
    if (inTauri) await invoke("library_end_session");
    else set({ session: null });
  },

  pick: (what: "program" | "image" | "emulator" | "folder") =>
    inTauri
      ? invoke<string | null>("library_pick", { what })
      : Promise.resolve(
          what === "program"
            ? "C:\\Users\\Carla\\Desktop\\Hookshot.lnk"
            : what === "emulator"
              ? "C:\\Emulators\\Dolphin-x64\\Dolphin.exe"
              : what === "folder"
                ? "D:\\My Games\\GameCube"
                : null,
        ),

  async consoles(): Promise<Console[]> {
    return inTauri ? invoke<Console[]>("consoles_list") : previewConsoles;
  },

  /** What to fill in for an emulator CarlaOS recognises by its name. */
  async suggestConsole(emulator: string): Promise<ConsoleHint> {
    if (inTauri) return invoke<ConsoleHint>("consoles_suggest", { emulator });
    return { name: "GameCube and Wii", options: '-b -e "{game}"', endings: ["iso", "gcm", "rvz", "wbfs"] };
  },

  /** Add or change a console. Resolves with how many games it has. */
  async saveConsole(console_: Console): Promise<number> {
    if (inTauri) {
      const games = await invoke<number>("consoles_save", { console: console_ });
      await reload();
      return games;
    }
    if (!console_.endings.length) throw "Say which file endings are games, for example iso or chd.";
    const saved = { ...console_, id: console_.id || `c${previewConsoles.length + 1}` };
    previewConsoles = [...previewConsoles.filter((c) => c.id !== saved.id), saved];
    return 3;
  },

  async removeConsole(id: string) {
    if (inTauri) {
      await invoke("consoles_remove", { id });
      await reload();
    } else previewConsoles = previewConsoles.filter((c) => c.id !== id);
  },

  /** Add a game by hand. Resolves with its id. */
  async addManual(game: NewGame): Promise<string> {
    if (inTauri) {
      const id = await invoke<string>("library_add_manual", { game });
      await reload();
      return id;
    }
    const now = new Date().toISOString();
    const added: Game = {
      id: `manual:${game.exe.toLowerCase()}`,
      title: game.title,
      description: game.description,
      platform: "Standalone",
      origin: game.origin || undefined,
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
    return added.id;
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
    if (!game) return { achievements: [], achievementsFrom: null, milestones: [], saves: null, sync: [], sessions: 0 };
    return { ...sampleDetails(game), saves: preview(gameId) };
  },

  /** Bring a game's saves in step with the desktops now. One line per desktop that takes part. */
  async syncSaves(gameId: string): Promise<SaveSync[]> {
    if (inTauri) return invoke<SaveSync[]>("saves_sync", { gameId });
    const game = state.games.find((g) => g.id === gameId);
    const at = Math.floor(Date.now() / 1000);
    return game ? sampleDetails(game).sync.map((s) => ({ ...s, at, outcome: "same" as const, files: 0 })) : [];
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

  /** What CarlaOS makes of a program or shortcut that was chosen. Administrator only. */
  async inspect(path: string): Promise<Inspected> {
    if (inTauri) return invoke<Inspected>("library_inspect", { path });
    // The preview knows one port, to show what recognising one looks like.
    const port = /hookshot|soh/i.test(path);
    return {
      exe: path.replace(/\.lnk$/i, ".exe"),
      args: "",
      title: port ? "The Legend of Zelda: Ocarina of Time" : previewTitle(path),
      kind: port ? "port" : "game",
      origin: port ? "Nintendo 64" : "",
      port: port ? "Hookshot" : "",
      shortcut: /\.lnk$/i.test(path),
    };
  },

  /** Look for pictures of a game, several to choose from. Administrator only. */
  async findArt(gameId: string, term: string): Promise<ArtSearch> {
    if (inTauri) {
      const found = await invoke<{ candidates: ArtChoice[]; notes: string[]; key_set: boolean }>("art_find", { gameId, term });
      return { candidates: found.candidates.map((c) => ({ ...c, preview: convertFileSrc(c.preview) })), notes: found.notes, keySet: found.key_set };
    }
    await new Promise((r) => setTimeout(r, 400));
    const game = state.games.find((g) => g.id === gameId);
    const all = game ? sampleArt(game, term) : [];
    return { candidates: previewKey ? all : all.filter((c) => c.candidate.source !== "SteamGridDB"), notes: [], keySet: previewKey };
  },

  /** Give a game one of the pictures that were found. */
  async useArt(gameId: string, choice: ArtChoice) {
    if (inTauri) {
      await invoke("art_use", { gameId, candidate: choice.candidate });
      await reload();
      return;
    }
    const wide = choice.candidate.shape === "wide";
    set({ games: state.games.map((g) => (g.id === gameId ? { ...g, ownArt: true, ...(wide ? { hero: choice.preview } : { cover: choice.preview }) } : g)) });
  },

  /** Use a picture that is on the Ally. Resolves with whether one was chosen. */
  async pickArt(gameId: string, wide: boolean): Promise<boolean> {
    if (!inTauri) return false;
    const chosen = await invoke<boolean>("art_pick", { gameId, wide });
    if (chosen) await reload();
    return chosen;
  },

  /** Go back to the pictures the game had by itself. */
  async clearArt(gameId: string) {
    if (inTauri) {
      await invoke("art_clear", { gameId });
      await reload();
    } else set({ games: state.games.map((g) => (g.id === gameId ? { ...g, ownArt: false, cover: undefined, hero: undefined } : g)) });
  },

  /** Whether a SteamGridDB key is kept. */
  async artKey(): Promise<boolean> {
    return inTauri ? invoke<boolean>("art_key") : previewKey;
  },

  /** Keep a SteamGridDB key, or forget it when empty. Resolves with whether one is kept. */
  async setArtKey(key: string): Promise<boolean> {
    if (inTauri) return invoke<boolean>("art_set_key", { key });
    if (key && !/^[a-z0-9]{16,64}$/i.test(key)) throw "That does not look like a SteamGridDB key.";
    return (previewKey = !!key);
  },

  /** Look a game up on Steam and Wikipedia by name. Administrator only. */
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
    return { name, description: "A hero wakes in a forest, finds a sword and a song, and sets out across a kingdom and through time itself.", cover: null, preview: null };
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
