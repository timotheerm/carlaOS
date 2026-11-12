import type { CloudService, Game, Host } from "./types";

// Invented titles on purpose: the prototype must not ship or imply real artwork.
export const hosts: Host[] = [
  {
    id: "home",
    name: "HOME PC",
    state: "ready",
    route: "local",
    latencyMs: 4,
    hardware: "Ryzen 7 9800X3D · RX 9070 XT · 64 GB",
    lastSeen: "now",
  },
  {
    id: "second",
    name: "SECOND PC",
    state: "asleep",
    route: null,
    latencyMs: null,
    hardware: "Not reported yet",
    lastSeen: "yesterday, 22:14",
    canWake: true,
  },
];

const g = (
  id: string,
  title: string,
  platform: string,
  kind: Game["kind"],
  hue: number,
  places: string[],
  playtimeMinutes: number,
  lastPlayed: string | undefined,
  added: string,
  description: string,
): Game => ({ id, title, platform, kind, hue, places, playtimeMinutes, lastPlayed, added, description, favourite: false });

export const games: Game[] = [
  g("neon-harbor", "Neon Harbor", "Steam", "game", 330, ["local", "home"], 1260, "2026-11-10", "2026-09-02",
    "A rain-soaked detective story set across one endless night in a floating city."),
  g("petal-kart", "Petal Kart", "Steam", "game", 345, ["local"], 420, "2026-11-09", "2026-10-11",
    "Bright, quick kart racing with tracks that bloom as you drive."),
  g("starfall-odyssey", "Starfall Odyssey", "Epic", "game", 265, ["home", "second"], 3310, "2026-11-08", "2026-06-20",
    "An open-galaxy role-playing game about a crew chasing a falling star."),
  g("hollow-tides", "Hollow Tides", "GOG", "game", 195, ["home"], 95, "2026-11-02", "2026-11-01",
    "Sail between drowned islands and piece together who sank them."),
  g("iron-meadow", "Iron Meadow", "Xbox", "game", 120, ["local", "home"], 610, "2026-10-28", "2026-08-15",
    "A gentle farming game on the back of a sleeping machine."),
  g("ashen-crown", "Ashen Crown", "Steam", "game", 15, ["home", "second"], 2240, "2026-10-20", "2026-03-04",
    "A demanding action game through the ruins of a burnt kingdom."),
  g("paper-lanterns", "Paper Lanterns", "Standalone", "game", 40, ["local"], 180, "2026-10-14", "2026-10-13",
    "A short hand-drawn puzzle game about lighting the way home."),
  g("velvet-circuit", "Velvet Circuit", "Epic", "game", 300, ["home"], 0, undefined, "2026-11-10",
    "Rhythm racing through a city that pulses to the music."),
  g("cloudberry", "Cloudberry", "itch.io", "homebrew", 355, ["local"], 35, "2026-09-30", "2026-09-29",
    "A tiny platformer made in a weekend, about a berry who wants to fly."),
  g("moss-and-ember", "Moss & Ember", "Switch", "emulated", 150, ["local", "home"], 540, "2026-11-05", "2026-07-07",
    "Two spirits, one forest, and a co-operative adventure through the seasons."),
  g("comet-crew", "Comet Crew", "Switch", "emulated", 215, ["home"], 75, "2026-10-01", "2026-09-12",
    "Party games in zero gravity for up to four players."),
  g("tin-knight", "Tin Knight", "GameCube", "emulated", 235, ["local"], 300, "2026-08-22", "2026-05-18",
    "A wind-up knight explores a toy-box kingdom."),
  g("lowpoly-rally", "Lowpoly Rally", "PlayStation 2", "emulated", 25, ["local"], 60, "2026-07-30", "2026-07-29",
    "Arcade rally racing with chunky cars and big jumps."),
  g("ocean-sixty-four", "Harbour 64", "Native port", "port", 205, ["local"], 410, "2026-10-25", "2026-10-02",
    "A community-built native version of a classic seaside adventure."),
  g("open-quarry", "Open Quarry", "Native port", "port", 30, ["local", "home"], 15, "2026-09-05", "2026-09-05",
    "An open-source reimplementation of a beloved mining shooter."),
];

export const cloud: CloudService[] = [
  { id: "gfn", name: "GeForce NOW", note: "Rented cloud PC", hue: 110 },
  { id: "xcloud", name: "Xbox Cloud Gaming", note: "Game Pass in the cloud", hue: 130 },
  { id: "boosteroid", name: "Boosteroid", note: "Rented cloud PC", hue: 210 },
];
