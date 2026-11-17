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

// A drawn scene to stand in for a game's wide picture or cover in the browser
// preview: sky, sun and hills in the game's colour. Invented, like the games.
function scene(hue: number, tall: boolean, title?: string): string {
  const [w, h] = tall ? [600, 900] : [1920, 1080];
  const sun = { x: w * (0.62 + (hue % 30) / 100), y: h * (0.3 + (hue % 17) / 100), r: h * 0.13 };
  const hill = (i: number) => {
    const base = h * (0.52 + i * 0.13);
    const a = h * (0.1 - i * 0.02);
    let d = `M0 ${h} L0 ${base}`;
    for (let x = 0; x <= w; x += w / 12) d += ` L${x.toFixed(0)} ${(base + Math.sin(x / (w / (5 + i * 2)) + hue + i * 2) * a - Math.cos(x / (w / 3) + i) * a * 0.6).toFixed(0)}`;
    return `<path d="${d} L${w} ${h} Z" fill="hsl(${(hue + 320 + i * 12) % 360} ${44 - i * 4}% ${46 - i * 11}%)"/>`;
  };
  const stars = Array.from({ length: 26 }, (_, i) => `<circle cx="${((i * 397 + hue * 7) % w).toFixed(0)}" cy="${((i * 211 + hue * 3) % (h * 0.5)).toFixed(0)}" r="${1.5 + (i % 3)}" fill="#fff" opacity="${0.25 + (i % 4) * 0.12}"/>`).join("");
  const name = title ? `<text x="${w * 0.08}" y="${h * 0.9}" font-family="Nunito, Segoe UI, sans-serif" font-weight="800" font-size="62" fill="#fff">${title.replace(/&/g, "and").slice(0, 15)}</text>` : "";
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" preserveAspectRatio="xMidYMid slice"><defs><linearGradient id="s" x1="0" y1="0" x2="0.3" y2="1"><stop offset="0" stop-color="hsl(${(hue + 300) % 360} 55% 34%)"/><stop offset="0.6" stop-color="hsl(${hue} 70% 62%)"/><stop offset="1" stop-color="hsl(${(hue + 30) % 360} 85% 78%)"/></linearGradient><radialGradient id="g"><stop offset="0" stop-color="#fff" stop-opacity="0.95"/><stop offset="0.35" stop-color="hsl(${(hue + 30) % 360} 100% 88%)" stop-opacity="0.7"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></radialGradient></defs><rect width="${w}" height="${h}" fill="url(#s)"/>${stars}<circle cx="${sun.x.toFixed(0)}" cy="${sun.y.toFixed(0)}" r="${(sun.r * 2.6).toFixed(0)}" fill="url(#g)"/>${hill(0)}${hill(1)}${hill(2)}${name}</svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

// Which sample games have no wide picture, or no picture at all, so the
// preview shows what those look like too.
const NO_WIDE = new Set(["tin-knight", "velvet-circuit", "thunder-lane"]);
const NO_PICTURE = new Set(["paper-lanterns", "cloudberry", "lantern-keepers"]);

// One game whose desktop has an update to finish, so the preview shows it.
const waiting: Record<string, Game["updates"]> = {
  "ashen-crown": [{ place: "home", state: "needed", bytes: 2_400_000_000, done: 0 }],
  "iron-meadow": [{ place: "local", state: "needed", bytes: 850_000_000, done: 0 }],
};

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
  g("cloud-skyline", "Skyline Drift", "Xbox Cloud Gaming", "game", 285, ["cloud"], 130, "2026-11-07", "2026-10-30",
    "Open-road racing across a coastline that never ends."),
  g("cloud-ember", "Ember Guard", "GeForce NOW", "game", 10, ["cloud"], 0, undefined, "2026-11-12",
    "A tower-defence game where the towers are dragons."),
  g("open-quarry", "Open Quarry", "Standalone", "port", 30, ["local", "home"], 15, "2026-09-05", "2026-09-05",
    "An open-source reimplementation of a beloved mining shooter."),
] as Game[]).map((game) => ({
  ...game,
  updates: waiting[game.id] ?? [],
  cover: NO_PICTURE.has(game.id) ? undefined : scene(game.hue, true, game.title),
  hero: NO_PICTURE.has(game.id) || NO_WIDE.has(game.id) ? undefined : scene(game.hue, false),
}));

export const cloud: CloudService[] = [
  { id: "gfn", name: "GeForce NOW", note: "Your own PC games, on a rented cloud PC", hue: 110, kind: "Cloud PC" },
  { id: "xcloud", name: "Xbox Cloud Gaming", note: "Game Pass games, streamed", hue: 130, kind: "Game Pass" },
  { id: "boosteroid", name: "Boosteroid", note: "Your own PC games, on a rented cloud PC", hue: 210, kind: "Cloud PC" },
  { id: "luna", name: "Amazon Luna", note: "Luna+ and Prime games, streamed", hue: 270, kind: "Channels" },
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
