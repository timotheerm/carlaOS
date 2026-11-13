// Shapes the UI works with. They mirror the catalogue tables planned for the
// Rust core (`games`, `installs`, `hosts`), so swapping sample data for the
// real catalogue in M2 does not touch the screens.

export type Place = "local" | string; // "local" or a host id

export type Kind = "game" | "emulated" | "port" | "homebrew";

export type Game = {
  id: string;
  title: string;
  description: string;
  platform: string; // Steam, Epic, Standalone, Switch, GameCube...
  kind: Kind;
  hue: number; // placeholder cover colour until real artwork arrives
  places: Place[]; // where it is installed
  playtimeMinutes: number;
  lastPlayed?: string; // ISO date
  added: string; // ISO date
  favourite: boolean;
  cover?: string; // picture URL, when there is real artwork
  hero?: string;
  manual?: boolean; // added by hand, so it can be removed
};

export type Session = { gameId: string; title: string; phase: "waking" | "starting" | "running"; stream: boolean };

export type NewGame = { title: string; exe: string; args: string; kind: Kind; cover: string | null; description: string };

export type Achievement = {
  id: string;
  name: string;
  description: string;
  hidden: boolean; // the game keeps it secret until it is earned
  unlocked: number | null; // Unix seconds; 0 when earned at an unknown time
};

export type Backup = { id: string; at: number; files: number; bytes: number };

export type Saves = { folder: string; files: number; bytes: number; modified: number | null; backups: Backup[] };

/// What the page of one game shows before it starts.
export type Details = {
  achievements: Achievement[]; // the game's own
  achievementsFrom: string | null; // "steam" or a desktop's name
  milestones: Achievement[]; // CarlaOS's own, which every game has
  saves: Saves | null; // when CarlaOS knows where they are
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
export type Match = { id: string; name: string };

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
};

/// A companion found on the network, not linked yet.
export type FoundHost = { address: string; name: string; id: string };

export type CloudService = { id: string; name: string; note: string; hue: number };
