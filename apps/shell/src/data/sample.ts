import type { Achievement, ArtChoice, CloudService, Details, Game, Host, Match } from "./types";

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
    version: "0.5.0",
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
  origin?: string,
  installable: string[] = [],
): Game => ({ id, title, platform, kind, hue, places, installable, playtimeMinutes, lastPlayed, added, description, favourite: false, origin });

// One game whose desktop has an update to finish, so the preview shows it.
const waiting: Record<string, Game["updates"]> = { "ashen-crown": [{ place: "home", state: "needed", bytes: 2_400_000_000, done: 0 }] };

export const games: Game[] = ([
  g("neon-harbor", "Neon Harbor", "Steam", "game", 330, ["local", "home"], 1260, "2026-11-10", "2026-09-02",
    "A rain-soaked detective story set across one endless night in a floating city."),
  g("petal-kart", "Petal Kart", "Steam", "game", 345, ["local"], 420, "2026-11-09", "2026-10-11",
    "Bright, quick kart racing with tracks that bloom as you drive."),
  g("starfall-odyssey", "Starfall Odyssey", "Epic", "game", 265, ["home", "second"], 3310, "2026-11-08", "2026-06-20",
    "An open-galaxy role-playing game about a crew chasing a falling star."),
  g("glass-garden", "Glass Garden", "Steam", "game", 170, [], 0, undefined, "2026-11-12",
    "Grow a greenhouse on a comet and keep it alive through the long dark.", undefined, ["local", "home"]),
  g("thunder-lane", "Thunder Lane", "Steam", "game", 225, [], 0, undefined, "2026-11-12",
    "Arcade bowling in a storm, with lanes that tilt in the wind.", undefined, ["local", "home"]),
  g("sugar-rush-saga", "Sugar Rush Saga", "Steam", "game", 320, [], 0, undefined, "2026-11-12",
    "A candy-coloured platformer with a hundred tiny levels.", undefined, ["local"]),
  g("lantern-keepers", "Lantern Keepers", "Steam", "game", 50, [], 0, undefined, "2026-11-12",
    "A co-operative lighthouse mystery for two.", undefined, ["local", "home", "second"]),
  g("hollow-tides", "Hollow Tides", "GOG", "game", 195, ["home"], 95, "2026-11-02", "2026-11-01",
    "Sail between drowned islands and piece together who sank them."),
  g("iron-meadow", "Iron Meadow", "Xbox", "game", 120, ["local", "home"], 610, "2026-10-28", "2026-08-15",
    "A gentle farming game on the back of a sleeping machine."),
  g("ashen-crown", "Ashen Crown", "Steam", "game", 15, ["home", "second"], 2240, "2026-10-20", "2026-03-04",
    "A demanding action game through the ruins of a burnt kingdom.", undefined, ["local"]),
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
  g("ocean-sixty-four", "Harbour 64", "Standalone", "port", 205, ["local"], 410, "2026-10-25", "2026-10-02",
    "A community-built native version of a classic seaside adventure.", "Nintendo 64"),
  g("open-quarry", "Open Quarry", "Standalone", "port", 30, ["local", "home"], 15, "2026-09-05", "2026-09-05",
    "An open-source reimplementation of a beloved mining shooter."),
] as Game[]).map((game) => ({ ...game, updates: waiting[game.id] ?? [] }));

export const cloud: CloudService[] = [
  { id: "gfn", name: "GeForce NOW", note: "Your own PC games, on a rented cloud PC", hue: 110, kind: "Cloud PC" },
  { id: "xcloud", name: "Xbox Cloud Gaming", note: "Game Pass games, streamed", hue: 130, kind: "Game Pass" },
  { id: "boosteroid", name: "Boosteroid", note: "Your own PC games, on a rented cloud PC", hue: 210, kind: "Cloud PC" },
];

// ---- what the browser preview shows on a game's page ----

const DAY = 86_400;
const at = (iso: string | undefined, daysBefore = 0) =>
  iso ? Math.floor(new Date(iso).getTime() / 1000) - daysBefore * DAY : null;

const OWN: [string, string, boolean][] = [
  ["Opening night", "Finish the opening chapter.", false],
  ["Collector", "Find 50 hidden things.", false],
  ["Night owl", "Play past midnight in the story.", false],
  ["Good company", "Bring every friend along.", false],
  ["Untouchable", "Clear a chapter without a scratch.", false],
  ["The long way round", "Take every detour.", false],
  ["A secret", "", true],
  ["Another secret", "", true],
  ["Completionist", "See everything there is to see.", false],
  ["The end", "Watch the credits.", false],
];

const MILESTONES: [string, string, string, number][] = [
  ["first", "First steps", "Play for the first time.", 1],
  ["time:1", "Settling in", "Play for one hour.", 60],
  ["time:5", "Hooked", "Play for five hours.", 300],
  ["time:10", "Regular", "Play for ten hours.", 600],
  ["time:25", "Devoted", "Play for twenty-five hours.", 1500],
  ["time:50", "True fan", "Play for fifty hours.", 3000],
  ["time:100", "Part of the family", "Play for a hundred hours.", 6000],
  ["days:3", "Back again", "Play on three different days.", 200],
  ["days:7", "A week of it", "Play on seven different days.", 500],
  ["days:30", "Old friends", "Play on thirty different days.", 2500],
  ["marathon", "Marathon", "Play for three hours in one sitting.", 900],
];

/** Invented details for a sample game, steady from one look to the next. */
export function sampleDetails(game: Game): Details {
  const played = game.playtimeMinutes;
  const steam = game.platform === "Steam" || game.platform === "Epic";
  const earned = Math.min(OWN.length, Math.floor(played / 240));
  const achievements: Achievement[] = steam
    ? OWN.map(([name, description, hidden], i) => ({
        id: `a${i}`,
        name,
        description,
        hidden,
        unlocked: i < earned ? at(game.lastPlayed, (earned - i) * 3) : null,
      }))
    : [];
  const milestones: Achievement[] = MILESTONES.map(([id, name, description, needed], i) => ({
    id,
    name,
    description,
    hidden: false,
    unlocked: played >= needed || (needed === 1 && !!game.lastPlayed) ? at(game.lastPlayed, (MILESTONES.length - i) * 2) : null,
  }));
  const local = game.places.includes("local");
  const modified = at(game.lastPlayed);
  return {
    achievements,
    achievementsFrom: steam ? (local ? "steam" : "HOME PC") : null,
    milestones,
    saves:
      local && modified !== null && game.kind !== "emulated"
        ? {
            folder: `C:\\Users\\Carla\\Saved Games\\${game.title}`,
            files: 4,
            bytes: 2_600_000,
            modified,
            backups: [
              { id: String(modified), at: modified, files: 4, bytes: 2_600_000 },
              { id: String(modified - 3 * DAY), at: modified - 3 * DAY, files: 4, bytes: 2_450_000 },
            ],
          }
        : null,
    // One sample game shows what keeping saves in step looks like.
    sync: local && modified !== null && game.kind !== "emulated" && game.platform !== "Steam" && game.places.length > 1 ? [{ host: "HOME PC", at: modified + 60, outcome: "sent", both_changed: false, files: 2, problem: null }] : [],
    sessions: played === 0 ? 0 : Math.max(1, Math.round(played / 70)),
  };
}

export function sampleMatches(term: string): Match[] {
  const all = [
    { id: "steam:101", name: "Paper Lanterns", from: "Steam" },
    { id: "steam:102", name: "Paper Lanterns: The Long Night", from: "Steam" },
    { id: "wiki:103", name: "Lantern Keeper", from: "Wikipedia" },
  ];
  const words = term.toLowerCase().split(/\s+/).filter(Boolean);
  const hits = words.length ? all.filter((m) => words.some((w) => w.length > 3 && m.name.toLowerCase().includes(w))) : all;
  // Anything else is "found" under its own name, so the form can be tried.
  return hits.length ? hits : [{ id: "wiki:900", name: term.trim(), from: "Wikipedia" }, { id: "steam:901", name: `${term.trim()} HD`, from: "Steam" }];
}

// ---- pictures the browser preview offers to choose from ----

// A drawn stand-in for a cover, so the picture chooser can be tried without
// fetching anything: invented, like the games.
function drawn(title: string, hue: number, wide: boolean, variant: number): string {
  const [w, h] = wide ? [920, 430] : [600, 900];
  const a = (hue + variant * 47) % 360;
  const b = (a + 40 + variant * 13) % 360;
  const shapes = Array.from({ length: 5 }, (_, i) => {
    const x = ((i * 173 + variant * 91) % w).toFixed(0);
    const y = ((i * 239 + variant * 57) % h).toFixed(0);
    const r = 60 + ((i * 53 + variant * 31) % 140);
    return `<circle cx="${x}" cy="${y}" r="${r}" fill="hsl(${(a + i * 24) % 360} 80% ${62 + i * 5}%)" opacity="0.35"/>`;
  }).join("");
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="hsl(${a} 68% 70%)"/><stop offset="1" stop-color="hsl(${b} 48% 34%)"/></linearGradient></defs><rect width="${w}" height="${h}" fill="url(#g)"/>${shapes}<text x="${w * 0.08}" y="${h * 0.86}" font-family="Nunito, Segoe UI, sans-serif" font-weight="800" font-size="${wide ? 64 : 68}" fill="#fff">${title.replace(/&/g, "and").slice(0, 16)}</text></svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

export function sampleArt(game: Game, term: string): ArtChoice[] {
  const title = term.trim() || game.title;
  const from = ["SteamGridDB", "Wikipedia", "Steam", "Box art", "SteamGridDB", "Steam", "SteamGridDB", "Wikipedia"];
  return from.map((source, i) => {
    const wide = i >= 6;
    const picture = drawn(title, game.hue, wide, i);
    return { candidate: { source, label: title, shape: wide ? "wide" : "tall", thumb: picture, full: picture }, preview: picture };
  });
}
