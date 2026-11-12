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
};

export type HostState = "ready" | "busy" | "not_ready" | "asleep" | "off" | "unreachable";

export type Host = {
  id: string;
  name: string;
  state: HostState;
  route: "local" | "remote" | "relayed" | null;
  latencyMs: number | null;
  hardware: string;
  lastSeen: string;
};

export type CloudService = { id: string; name: string; note: string; hue: number };
