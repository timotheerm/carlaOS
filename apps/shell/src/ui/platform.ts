// The band along the bottom of a game's picture that says which console it
// is from, the way a game's box does. Games from a PC store have none: their
// badge already says where they are from.
import type { Game } from "../data/types";

export type Banner = { label: string; colour: string; ink: string };

// (words in the console's name as the administrator typed it, colour of the
// band). Longer names first, so "Game Boy Advance" is not taken for a Game Boy.
const CONSOLES: [string[], string, string?][] = [
  [["switch"], "#e60012"],
  [["wii u", "wiiu"], "#0096c8"],
  [["wii"], "#e9edf0", "#3d4a52"],
  [["gamecube", "game cube"], "#5b4bb5"],
  [["nintendo 64", "n64"], "#1d9a55"],
  [["super nintendo", "snes", "super famicom"], "#7463b8"],
  [["game boy advance", "gba"], "#4a43a8"],
  [["game boy color", "gbc"], "#8a3fb3"],
  [["game boy"], "#7a8c2e"],
  [["3ds"], "#cf2029"],
  [["nintendo ds", "nds"], "#5d6b7c"],
  [["nes", "famicom"], "#b8323a"],
  [["playstation portable", "psp"], "#22232b"],
  [["playstation", "ps1", "ps2", "ps3", "psx"], "#0b53b5"],
  [["xbox"], "#107c10"],
  [["dreamcast"], "#ee7a22"],
  [["saturn"], "#2c4b8e"],
  [["mega drive", "megadrive", "genesis", "master system"], "#1f2026"],
];

const PORT = "#7a4fb5";
const HOMEBREW = "#2f8f83";
const OTHER = "#a8456a";

/** The colour of a console's band, going by its name. */
export function consoleColour(name: string): { colour: string; ink: string } | null {
  const spaced = ` ${name.toLowerCase()} `;
  for (const [words, colour, ink] of CONSOLES) {
    // Whole words only: there is no NES in "Genesis".
    if (words.some((w) => spaced.includes(` ${w} `))) return { colour, ink: ink ?? "#ffffff" };
  }
  return null;
}

/** The band for one game, or nothing when it needs none. */
export function banner(game: Game): Banner | null {
  if (game.kind === "emulated") {
    const found = consoleColour(game.platform);
    return { label: game.platform, colour: found?.colour ?? OTHER, ink: found?.ink ?? "#ffffff" };
  }
  if (game.kind === "port") {
    const found = game.origin ? consoleColour(game.origin) : null;
    return {
      label: game.origin ? `${game.origin} · PC port` : "PC port",
      colour: found?.colour ?? PORT,
      ink: found?.ink ?? "#ffffff",
    };
  }
  if (game.kind === "homebrew") return { label: "Homebrew", colour: HOMEBREW, ink: "#ffffff" };
  return null;
}
