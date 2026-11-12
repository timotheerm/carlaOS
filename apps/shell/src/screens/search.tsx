// Search by name. A controller types on the letters below; a keyboard, or the
// Windows touch keyboard, types straight into the field.
import { Empty, GameCard, Grid } from "../ui/parts";
import type { Ctx } from "./screens";

const ROWS = ["ABCDEFGHIJ", "KLMNOPQRST", "UVWXYZ0123", "456789"];

// "Pokémon" is found by "pokemon", and "Moss & Ember" by "moss ember".
const plain = (text: string) =>
  text
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ")
    .trim();

export function matches(
  title: string,
  platform: string,
  query: string,
): boolean {
  const words = plain(query).split(" ").filter(Boolean);
  const hay = ` ${plain(title)} ${plain(platform)}`;
  // Every word typed must begin a word of the name, so "ha" finds "Harbor"
  // but not "Orchard".
  return words.length > 0 && words.every((w) => hay.includes(` ${w}`));
}

// The words typed live above this page, so they are still there when the
// player comes back from looking at a game.
export function Search({
  query,
  setQuery,
  ...ctx
}: Ctx & { query: string; setQuery: (update: (q: string) => string) => void }) {
  const found = ctx.games
    .filter((g) => matches(g.title, g.platform, query))
    .sort((a, b) => a.title.localeCompare(b.title));
  const type = (letter: string) => setQuery((q) => (q + letter).slice(0, 40));

  return (
    <div className="search">
      <div className="search__type">
        <input
          className="field search__field"
          data-focus
          data-fid="search:field"
          value={query}
          maxLength={40}
          placeholder="Type the name of a game"
          aria-label="Search"
          onChange={(e) => setQuery(() => e.target.value)}
        />
        <div className="keys" role="group" aria-label="Letters">
          {ROWS.map((row, r) => (
            <div className="keys__row" key={row}>
              {row.split("").map((letter, i) => (
                <button
                  key={letter}
                  className="key"
                  data-focus
                  data-autofocus={r === 0 && i === 0 ? "" : undefined}
                  data-fid={`key:${letter}`}
                  onClick={() => type(letter.toLowerCase())}
                >
                  {letter}
                </button>
              ))}
              {r === ROWS.length - 1 && (
                <>
                  <button
                    className="key key--wide"
                    data-focus
                    data-fid="key:space"
                    onClick={() => type(" ")}
                  >
                    Space
                  </button>
                  <button
                    className="key key--wide"
                    data-focus
                    data-fid="key:delete"
                    onClick={() => setQuery((q) => q.slice(0, -1))}
                  >
                    Delete
                  </button>
                  <button
                    className="key key--wide"
                    data-focus
                    data-fid="key:clear"
                    onClick={() => setQuery(() => "")}
                  >
                    Clear
                  </button>
                </>
              )}
            </div>
          ))}
        </div>
      </div>
      {plain(query) === "" ? (
        <Empty>
          Search looks through every game, on this Ally and on your desktops.
        </Empty>
      ) : found.length ? (
        <Grid>
          {found.map((g) => (
            <GameCard
              key={g.id}
              game={g}
              favourite={ctx.favourites.has(g.id)}
              onOpen={ctx.openGame}
            />
          ))}
        </Grid>
      ) : (
        <Empty>No game is called that.</Empty>
      )}
    </div>
  );
}
