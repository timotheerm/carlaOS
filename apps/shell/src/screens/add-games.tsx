// Add games that CarlaOS already knows about to this Ally's personal library.
import { useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Game } from "../data/types";
import { Cover } from "../ui/parts";
import { focusById, focusFirst } from "../nav/focus";
import { matches, ROWS } from "./search";
import type { Ctx } from "./screens";

const FILTERS = [
  { id: "all", label: "All games" },
  { id: "local", label: "On this Ally" },
  { id: "desktop", label: "On desktops" },
  { id: "absent", label: "Not installed" },
] as const;

function availability(game: Game, hosts: Ctx["hosts"]): string {
  if (game.places.includes("local")) return "Installed on this Ally";
  const desktops = game.places.filter((p) => p !== "local" && p !== "cloud");
  if (desktops.length) {
    const names = desktops.map((id) => hosts.find((h) => h.id === id)?.name ?? "A desktop");
    return "Installed on " + names.join(", ");
  }
  if (game.installable.includes("local")) return "Steam · Can be installed on this Ally";
  if (game.installable.length) return "Available to install on a desktop";
  if (game.places.includes("cloud")) return "Available in the cloud";
  return game.platform;
}

export function AddGames(ctx: Ctx) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<(typeof FILTERS)[number]["id"]>("all");
  const [adding, setAdding] = useState<string | null>(null);
  const [keyboardOpen, setKeyboardOpen] = useState(false);
  const preferredFocus = useRef<string | null>(null);
  const selected = useMemo(() => new Set(ctx.library.map((g) => g.id)), [ctx.library]);
  const matching = ctx.catalogue
    .filter((g) => !selected.has(g.id))
    .filter((g) => !query.trim() || matches(g.title, g.platform, query));
  const counts = {
    all: matching.length,
    local: matching.filter((g) => g.places.includes("local")).length,
    desktop: matching.filter((g) => g.places.some((p) => p !== "local" && p !== "cloud")).length,
    absent: matching.filter((g) => g.places.length === 0).length,
  };
  const candidates = matching
    .filter((g) =>
      filter === "local"
        ? g.places.includes("local")
        : filter === "desktop"
          ? g.places.some((p) => p !== "local" && p !== "cloud")
          : filter === "absent"
            ? g.places.length === 0
            : true,
    )
    .sort((a, b) => a.title.localeCompare(b.title));

  useLayoutEffect(() => {
    const active = document.activeElement;
    if (active instanceof HTMLElement && active.isConnected && active.matches("[data-focus]")) return;
    const wanted = preferredFocus.current;
    preferredFocus.current = null;
    if (wanted && focusById(wanted)) return;
    const main = document.querySelector("main");
    if (main) focusFirst(main);
  }, [selected.size, candidates.length]);

  useLayoutEffect(() => {
    if (keyboardOpen) focusById("add-key:A");
  }, [keyboardOpen]);

  const type = (letter: string) => setQuery((q) => (q + letter).slice(0, 60));

  const add = async (game: Game, index: number) => {
    const next = candidates[index + 1] ?? candidates[index - 1];
    preferredFocus.current = next ? "add-game:" + next.id : "add-games:search";
    setAdding(game.id);
    try {
      await ctx.addToLibrary(game.id);
      ctx.notify(game.title + " was added to My Library.");
    } finally {
      setAdding(null);
    }
  };

  return (
    <div className="addgame">
      <div className="library-heading">
        <div>
          <h1>Add games</h1>
          <p>Choose from your Ally and linked desktops. Adding a game does not install it.</p>
        </div>
      </div>
      <div className="addgame__searchrow">
        <input
          className="field addgame__search"
          data-focus
          data-autofocus
          data-fid="add-games:search"
          value={query}
          maxLength={60}
          placeholder="Search games"
          aria-label="Search games"
          onChange={(e) => setQuery(e.target.value)}
        />
        <button
          className="pill addgame__keyboard-toggle"
          data-focus
          data-fid="add-games:keyboard"
          aria-expanded={keyboardOpen}
          aria-controls="add-games-keyboard"
          onClick={() => setKeyboardOpen((open) => !open)}
        >
          {keyboardOpen ? "Hide keyboard" : "On-screen keyboard"}
        </button>
      </div>
      <div className="addgame__summary" aria-live="polite">
        <strong>{candidates.length} {candidates.length === 1 ? "game" : "games"}</strong>
        <span>available to add · {ctx.library.length} already in My Library</span>
      </div>
      <div className="chips addgame__filters" role="group" aria-label="Filter games to add">
        {FILTERS.map((item) => (
          <button
            key={item.id}
            className={filter === item.id ? "chip chip--on" : "chip"}
            data-focus
            data-fid={"add-filter:" + item.id}
            aria-pressed={filter === item.id}
            onClick={() => setFilter(item.id)}
          >
            {item.label}
            <span className="chip__count">{counts[item.id]}</span>
          </button>
        ))}
      </div>
      <div id="add-games-keyboard" className="addgame__keyboard" hidden={!keyboardOpen}>
        <div className="keys" role="group" aria-label="Search letters">
          {ROWS.map((row, index) => (
            <div className="keys__row" key={row}>
              {row.split("").map((letter) => (
                <button key={letter} className="key" data-focus data-fid={"add-key:" + letter} onClick={() => type(letter.toLowerCase())}>{letter}</button>
              ))}
              {index === ROWS.length - 1 && (
                <>
                  <button className="key key--wide" data-focus data-fid="add-key:space" onClick={() => type(" ")}>Space</button>
                  <button className="key key--wide" data-focus data-fid="add-key:delete" onClick={() => setQuery((q) => q.slice(0, -1))}>Delete</button>
                  <button className="key key--wide" data-focus data-fid="add-key:clear" onClick={() => setQuery("")}>Clear</button>
                </>
              )}
            </div>
          ))}
        </div>
      </div>
      <div className="addgame__list">
        {candidates.map((game, index) => (
          <article key={game.id} className="addgame__item">
            <div className="addgame__cover"><Cover game={game} band={false} /></div>
            <div className="addgame__info">
              <b>{game.title}</b>
              <span>{game.platform}</span>
              <small>{availability(game, ctx.hosts)}</small>
            </div>
            <button
              className="pill pill--primary addgame__button"
              data-focus
              data-fid={"add-game:" + game.id}
              aria-label={"Add " + game.title + " to My Library"}
              disabled={adding === game.id}
              onClick={() => void add(game, index).catch((e) => ctx.notify(typeof e === "string" ? e : "The game could not be added."))}
            >
              {adding === game.id ? "Adding…" : "Add"}
            </button>
          </article>
        ))}
        {candidates.length === 0 && (
          <div className="library-empty library-empty--small">
            <p>{query ? "No unselected games match that search." : "Everything CarlaOS currently knows about is already in My Library."}</p>
          </div>
        )}
      </div>
      <div className="addgame__footer">
        <p>To register a standalone program on this Ally, use Manage games. You can then add it here.</p>
        <button className="pill" data-focus data-fid="add-games:manage" onClick={ctx.openManage}>Add a game from this Ally</button>
      </div>
    </div>
  );
}
