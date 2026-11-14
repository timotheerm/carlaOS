// Shapes the UI works with. They mirror the catalogue tables planned for the
// Rust core (`games`, `installs`, `hosts`), so swapping sample data for the
// real catalogue in M2 does not touch the screens.

export type Place = "local" | "cloud" | string; // this Ally, a cloud service, or a desktop's id

export type Kind = "game" | "emulated" | "port" | "homebrew";

export type Game = {
  id: string;
  title: string;
  description: string;
  platform: string; // Steam, Epic, Standalone, Switch, GameCube...
  kind: Kind;
  hue: number; // placeholder cover colour until real artwork arrives
  places: Place[]; // where it is installed
  installable: Place[]; // where its store could install it and has not
  playtimeMinutes: number;
  lastPlayed?: string; // ISO date
  added: string; // ISO date
  favourite: boolean;
  cover?: string; // picture URL, when there is real artwork
  hero?: string;
  manual?: boolean; // added by hand, so it can be removed
  updates?: Waiting[]; // where a store has an update to finish first
  origin?: string; // the console a port first came out on: "Nintendo 64"
  ownArt?: boolean; // the administrator chose its pictures
};

/// A game its store is fetching, on this Ally or on a desktop.
export type Download = {
  gameId: string;
  place: Place;
  title: string;
  phase: "queued" | "downloading" | "paused" | "finishing";
  done: number; // bytes; both 0 until the store knows how much there is
  total: number;
  update: boolean; // an installed game being brought up to date
};

/// What fills the disks Steam installs to on this Ally.
export type Storage = {
  disks: { folder: string; free: number; total: number }[];
  games: { game_id: string; title: string; bytes: number; last_played: number | null }[];
};

/// An update a store has to finish in one place before the game starts there.
export type Waiting = {
  place: Place;
  state: "needed" | "running" | "paused";
  bytes: number; // to download in all; 0 when the store has not said
  done: number;
};

export type Session = { gameId: string; title: string; phase: "waking" | "starting" | "running"; stream: boolean; cloud: boolean; note?: string | null };

export type NewGame = { title: string; exe: string; args: string; kind: Kind; cover: string | null; description: string; origin: string };

/// What CarlaOS makes of a program or shortcut the administrator chose.
export type Inspected = {
  exe: string; // the program behind a shortcut, when it was one
  args: string;
  title: string;
  kind: Kind;
  origin: string;
  port: string; // what a port CarlaOS knows calls itself
  shortcut: boolean;
};

/// One picture found for a game, to choose from.
export type ArtChoice = {
  candidate: { source: string; label: string; shape: "tall" | "wide"; thumb: string; full: string };
  preview: string; // what to show: a small copy on this machine
};

export type ArtSearch = { candidates: ArtChoice[]; notes: string[]; keySet: boolean };

export type Achievement = {
  id: string;
  name: string;
  description: string;
  hidden: boolean; // the game keeps it secret until it is earned
  unlocked: number | null; // Unix seconds; 0 when earned at an unknown time
};

export type Backup = { id: string; at: number; files: number; bytes: number };

export type Saves = { folder: string; files: number; bytes: number; modified: number | null; backups: Backup[] };

/// How a game's saves stand with one desktop that keeps them in step.
export type SaveSync = {
  host: string; // the desktop's name
  at: number;
  outcome: "same" | "sent" | "fetched" | "problem";
  both_changed: boolean; // both had new saves; the older ones are in a safe copy
  files: number;
  problem: string | null;
};

/// What the page of one game shows before it starts.
export type Details = {
  achievements: Achievement[]; // the game's own
  achievementsFrom: string | null; // "steam" or a desktop's name
  milestones: Achievement[]; // CarlaOS's own, which every game has
  saves: Saves | null; // when CarlaOS knows where they are
  sync: SaveSync[]; // one line per desktop that keeps the saves in step
  sessions: number;
};

/// An emulator that is installed, paired with a folder of game files.
export type Console = {
  id: string; // empty for a new one
  name: string;
  emulator: string;
  options: string; // with {game} where the game file goes
  folder: string;
  endings: string[];
};

export type ConsoleHint = { name: string; options: string; endings: string[] };

/// A game found in the public store, for filling in details.
export type Match = { id: string; name: string; from: string };

export type HostState = "ready" | "busy" | "not_ready" | "asleep" | "off" | "unreachable" | "unknown";

export type Host = {
  id: string;
  name: string;
  state: HostState;
  route: "local" | "remote" | "relayed" | null;
  latencyMs: number | null;
  hardware: string;
  lastSeen: string;
  playing?: string | null; // what is running there, when busy
  problem?: string | null; // one line on what is wrong, when it needs attention
  address?: string;
  canWake?: boolean; // the desktop has said how to wake it
  version?: string | null; // the companion's version, once it has answered
};

/// The picture of streams from one desktop: what the administrator chose and
/// what the last stream got.
export type HostPicture = {
  setting: string; // "auto", "auto120" or a level
  settings: string[]; // every setting, in the order to step through
  last: {
    level: string;
    automatic: boolean;
    height: number;
    fps: number;
    mbps: number;
    latency_ms: number | null;
    link_kbps: number | null; // what the connection carried when measured
    away: boolean;
    at: number;
  } | null;
};

/// A companion found on the network, not linked yet.
export type FoundHost = { address: string; name: string; id: string };

export type CloudService = { id: string; name: string; note: string; hue: number; kind: string };

/** How hard the Ally works for a game. */
export type Power = "quiet" | "balanced" | "turbo";
export const POWERS: Power[] = ["quiet", "balanced", "turbo"];
export const POWER_TEXT: Record<Power, { name: string; short: string; about: string }> = {
  quiet: { name: "Quiet", short: "Longest battery", about: "The fan stays down and the battery lasts longest. Best for small games, streams and cloud games." },
  balanced: { name: "Balanced", short: "Most games", about: "Enough for most games, without draining the battery." },
  turbo: { name: "Turbo", short: "Most power", about: "Everything the Ally has, for the most demanding games. Best when plugged in." },
};
/** A game's power level and screen speed. */
export type PowerView = {
  power: Power;
  /** "smooth": as fast as the screen goes. "saver": 60 times a second. */
  screen: "smooth" | "saver";
  /** Chosen for this game, rather than what every game gets. */
  chosen: boolean;
  /** A stream picks its own screen speed. */
  setsScreen: boolean;
};
