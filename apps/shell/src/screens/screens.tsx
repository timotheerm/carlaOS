import { useLayoutEffect, useState } from "react";
import { focusFirst } from "../nav/focus";
import type { CloudService, Download, Game, Host } from "../data/types";
import { Cover, Empty, GameCard, Grid, HostCard, Row, playtime } from "../ui/parts";
import { StateLine, installState } from "../ui/install";
import type { Backdrop } from "../data/types";

export type Ctx = {
  /** Personal-library games that can be played now, here or from a desktop. */
  games: Game[];
  /** Every game selected for this Ally's personal library, installed or not. */
  library: Game[];
  /** The complete catalogue known to CarlaOS, used to select more games. */
  catalogue: Game[];
  /** What the stores are fetching, here and on the desktops. */
  downloads: Download[];
  /** Ask the store to install a game in one place. */
  install: (gameId: string, place: string) => void;
  hosts: Host[];
  favourites: Set<string>;
  openGame: (id: string) => void;
  openHost: (id: string) => void;
  notify: (text: string) => void;
  /** Start a game from one of the places it is installed. */
  play: (gameId: string, place: string) => void;
  /** Ask the desktops again how they are. */
  refreshHosts: () => void;
  /** Wake a sleeping desktop. */
  wakeHost: (id: string) => void;
  /** Open a cloud gaming service itself. */
  openCloud: (service: string, name: string) => void;
  /** Open the page of everything that is being installed. */
  openDownloads: () => void;
  addToLibrary: (id: string) => Promise<void>;
  openAddGames: () => void;
  openManage: () => void;
  removeGame: (id: string) => void;
};

const byRecent = (a: Game, b: Game) => (b.lastPlayed ?? "").localeCompare(a.lastPlayed ?? "");
const countOn = (games: Game[], place: string) => games.filter((g) => g.places.includes(place)).length;

function cards(list: Game[], ctx: Ctx) {
  return list.map((g) => (
    <GameCard key={g.id} game={g} favourite={ctx.favourites.has(g.id)} onOpen={ctx.openGame} downloads={ctx.downloads} />
  ));
}

export function Home(ctx: Ctx) {
  const played = ctx.games.filter((g) => g.lastPlayed).sort(byRecent);
  const [hero, ...recent] = played;
  const favourites = ctx.games.filter((g) => ctx.favourites.has(g.id));
  const added = [...ctx.games].sort((a, b) => b.added.localeCompare(a.added)).slice(0, 8);

  // One card per game, whichever machine is fetching it.
  const fetched = ctx.library.filter((g) => ctx.downloads.some((d) => d.gameId === g.id));

  if (ctx.games.length === 0 && fetched.length === 0) {
    return (
      <div className="library-empty">
        <div className="library-empty__mark">C</div>
        <h1>{ctx.library.length ? "Nothing ready to play yet" : "Your library starts here"}</h1>
        <p>{ctx.library.length
          ? "Install one of your selected games, or add games from a linked desktop."
          : "Games installed on this Ally through Steam appear automatically. Add games from your desktops or choose Steam games to install later."}</p>
        <button className="pill pill--primary" data-focus data-autofocus data-fid="home:add-games" onClick={ctx.openAddGames}>Add games</button>
        {ctx.library.length === 0 && <button className="pill" data-focus data-fid="home:manage" onClick={ctx.openManage}>Add a game from this Ally</button>}
      </div>
    );
  }

  return (
    <>
      {hero && (
        <button
          className="hero"
          data-focus
          data-autofocus
          data-fid={`hero:${hero.id}`}
          data-game={hero.id}
          onClick={() => ctx.openGame(hero.id)}
        >
          <span className="hero__text">
            <span className="hero__eyebrow">Continue playing</span>
            <span className="hero__title">{hero.title}</span>
            <span className="hero__meta">
              <StateLine state={installState(hero, ctx.downloads)} extra={`${hero.platform} · ${playtime(hero.playtimeMinutes)}`} />
            </span>
            <span className="hero__go">Open</span>
          </span>
          <span className="hero__cover">
            <Cover game={hero} band={false} />
          </span>
        </button>
      )}
      {ctx.hosts.length > 0 && (
        <Row title="My desktops">
          {ctx.hosts.map((h) => (
            <HostCard key={h.id} host={h} gameCount={countOn(ctx.games, h.id)} onOpen={ctx.openHost} />
          ))}
        </Row>
      )}
      {fetched.length > 0 && (
        <Row title="Installing now">
          {cards(fetched, ctx)}
          <button className="card card--more" data-focus data-fid="home:downloads" onClick={ctx.openDownloads}>
            <span className="card__more">See every step</span>
            <span className="card__meta">
              <span className="card__sub">Downloads</span>
            </span>
          </button>
        </Row>
      )}
      {recent.length > 0 && <Row title="Recently played">{cards(recent.slice(0, 8), ctx)}</Row>}
      {favourites.length > 0 && <Row title="Favourites">{cards(favourites, ctx)}</Row>}
      {added.length > 0 && <Row title="Recently added">{cards(added, ctx)}</Row>}
    </>
  );
}

const FILTERS = [
  { id: "all", label: "All" },
  { id: "local", label: "On this Ally" },
  { id: "stream", label: "Streamed" },
  { id: "cloud", label: "Cloud" },
  { id: "absent", label: "Not installed" },
  { id: "fav", label: "Favourites" },
] as const;

type LibrarySort = "recent" | "name" | "added" | "played";

export function Games(ctx: Ctx) {
  const [filter, setFilter] = useState<(typeof FILTERS)[number]["id"]>("all");
  const [sort, setSort] = useState<LibrarySort>("recent");
  const counts = {
    all: ctx.library.length,
    local: countOn(ctx.library, "local"),
    stream: ctx.library.filter((g) => g.places.some((p) => p !== "local" && p !== "cloud")).length,
    cloud: countOn(ctx.library, "cloud"),
    absent: ctx.library.filter((g) => g.places.length === 0).length,
    fav: ctx.library.filter((g) => ctx.favourites.has(g.id)).length,
  };
  const filtered = ctx.library.filter((g) =>
    filter === "local"
      ? g.places.includes("local")
      : filter === "stream"
        ? g.places.some((p) => p !== "local" && p !== "cloud")
        : filter === "cloud"
          ? g.places.includes("cloud")
          : filter === "absent"
            ? g.places.length === 0
            : filter === "fav"
              ? ctx.favourites.has(g.id)
              : true,
  );
  const list = [...filtered].sort((a, b) => {
    if (sort === "name") return a.title.localeCompare(b.title);
    if (sort === "added") return b.added.localeCompare(a.added) || a.title.localeCompare(b.title);
    if (sort === "played") return b.playtimeMinutes - a.playtimeMinutes || a.title.localeCompare(b.title);
    return (b.lastPlayed ?? "").localeCompare(a.lastPlayed ?? "") || b.added.localeCompare(a.added) || a.title.localeCompare(b.title);
  });

  return (
    <>
      <div className="library-heading">
        <div>
          <h1>My Library</h1>
          <p>{ctx.library.length} games in your collection · {counts.local} on this Ally · {counts.stream} on linked desktops</p>
        </div>
        <button className="pill pill--primary" data-focus data-autofocus data-fid="library:add" onClick={ctx.openAddGames}>Add games</button>
      </div>
      <div className="library-toolbar">
        <div className="chips" role="group" aria-label="Filter My Library">
          {FILTERS.map((f, i) => (
            <button
              key={f.id}
              className={f.id === filter ? "chip chip--on" : "chip"}
              data-focus
              data-autofocus={i === 0 ? "" : undefined}
              data-fid={"filter:" + f.id}
              aria-pressed={f.id === filter}
              onClick={() => setFilter(f.id)}
            >
              {f.label}
              <span className="chip__count">{counts[f.id]}</span>
            </button>
          ))}
        </div>
        <label className="library-sort">
          <span>Sort by</span>
          <select
            className="field"
            data-focus
            data-fid="library:sort"
            aria-label="Sort games"
            value={sort}
            onChange={(e) => setSort(e.target.value as LibrarySort)}
          >
            <option value="recent">Recently played</option>
            <option value="name">Name</option>
            <option value="added">Recently added</option>
            <option value="played">Most played</option>
          </select>
        </label>
      </div>
      <div className="library-summary" aria-live="polite">
        <span>Showing <strong>{list.length}</strong> of {ctx.library.length} games</span>
        {filter !== "all" && <span>{FILTERS.find((f) => f.id === filter)?.label}</span>}
      </div>
      {list.length ? (
        <Grid>{cards(list, ctx)}</Grid>
      ) : (
        <div className="library-empty library-empty--small">
          <p>{filter === "absent" ? "No selected games are waiting to be installed." : filter === "fav" ? "You have not marked any games as favourites yet." : "No games in this view yet."}</p>
          <button className="pill pill--primary" data-focus data-fid="library:add-empty" onClick={ctx.openAddGames}>Add games</button>
        </div>
      )}
    </>
  );
}

export function Desktops(ctx: Ctx) {
  if (ctx.hosts.length === 0) return <Empty>No desktops are linked yet. Link one under Settings, then Desktops.</Empty>;
  return (
    <div className="hosts">
      {ctx.hosts.map((h) => (
        <HostCard key={h.id} host={h} gameCount={countOn(ctx.games, h.id)} onOpen={ctx.openHost} large />
      ))}
    </div>
  );
}

export function HostView({ hostId, ...ctx }: Ctx & { hostId: string }) {
  const host = ctx.hosts.find((h) => h.id === hostId);
  // The Wake and Check buttons disappear when the desktop answers. Put focus
  // back on something so A always does something.
  useLayoutEffect(() => {
    const active = document.activeElement;
    if (active instanceof HTMLElement && active.isConnected && active.matches("[data-focus]")) return;
    const main = document.querySelector("main");
    if (main) focusFirst(main);
  }, [host?.state]);
  if (!host) return <Empty>This desktop is no longer set up.</Empty>;
  const list = ctx.games.filter((g) => g.places.includes(hostId)).sort((a, b) => a.title.localeCompare(b.title));
  const games = `${list.length} ${list.length === 1 ? "game" : "games"}`;
  const line: Record<Host["state"], string> = {
    ready: `${games} ready to stream`,
    busy: host.playing ? `Playing ${host.playing}` : "A game is running there",
    not_ready: host.problem ?? "Streaming is not working on this desktop.",
    unreachable: `Not answering. It may be off or asleep. Last seen ${host.lastSeen}.`,
    asleep: `Asleep. Last seen ${host.lastSeen}.`,
    off: `Off. Last seen ${host.lastSeen}.`,
    unknown: "Checking…",
  };
  const awake = host.state === "ready" || host.state === "busy";
  return (
    <>
      <div className="pagehead">
        <h1>{host.name}</h1>
        <p>{line[host.state]}</p>
        {!awake && host.canWake && host.state !== "not_ready" && (
          <button className="pill pill--primary" data-focus data-autofocus data-fid="host:wake" onClick={() => ctx.wakeHost(host.id)}>
            Wake {host.name}
          </button>
        )}
        {!awake && (
          <button
            className={host.canWake && host.state !== "not_ready" ? "pill" : "pill pill--primary"}
            data-focus
            data-autofocus={host.canWake && host.state !== "not_ready" ? undefined : ""}
            data-fid="host:check"
            onClick={ctx.refreshHosts}
          >
            Check again
          </button>
        )}
      </div>
      {list.length ? <Grid>{cards(list, ctx)}</Grid> : <Empty>No games were found on this desktop yet.</Empty>}
    </>
  );
}

export function Cloud({ services, ...ctx }: Ctx & { services: CloudService[] }) {
  const games = ctx.games.filter((g) => g.places.includes("cloud")).sort(byRecent);
  return (
    <>
      <p className="lede">Games that run in a data centre and play here over the internet. Choose a service to look around in it, or a game to play it straight away.</p>
      <div className="hosts hosts--cloud">
        {services.map((s, i) => (
          <button
            key={s.id}
            className="host host--large host--cloud"
            style={{ "--hue": s.hue } as React.CSSProperties}
            data-focus
            data-autofocus={i === 0 && games.length === 0 ? "" : undefined}
            data-fid={`cloud:${s.id}`}
            onClick={() => ctx.openCloud(s.id, s.name)}
          >
            <span className="cloud__kind">{s.kind}</span>
            <span className="host__name">{s.name}</span>
            <span className="host__line host__line--quiet">{s.note}</span>
            <span className="cloud__glow" aria-hidden />
          </button>
        ))}
      </div>
      {games.length > 0 ? (
        <Row title="Cloud games">{cards(games, ctx)}</Row>
      ) : (
        <p className="lede">Add a cloud game from Settings, Manage games, then select it under Add Games in My Library.</p>
      )}
    </>
  );
}

export function Settings({
  theme,
  onTheme,
  onCleanup,
  onManage,
  onDesktops,
  onUpdates,
  onStartup,
  onCrash,
  onLog,
  onStorage,
  onDownloads,
  downloading,
  backdrop,
  onBackdrop,
  sounds,
  onSounds,
  hostCount,
  onRefresh,
  refreshing,
  gameCount,
  onExit,
  version,
}: {
  onManage: () => void;
  onDesktops: () => void;
  onUpdates: () => void;
  onStartup: () => void;
  onCrash: () => void;
  onLog: () => void;
  onStorage: () => void;
  onDownloads: () => void;
  downloading: number;
  backdrop: Backdrop;
  onBackdrop: () => void;
  sounds: boolean;
  onSounds: () => void;
  hostCount: number;
  onRefresh: () => void;
  refreshing: boolean;
  gameCount: number;
  theme: "light" | "dark";
  onTheme: () => void;
  onCleanup: () => void;
  onExit: () => void;
  version: string;
}) {
  return (
    <div className="settings">
      <button className="setting" data-focus data-autofocus data-fid="set:theme" onClick={onTheme}>
        <span>Appearance</span>
        <span className="setting__value">{theme === "light" ? "Light" : "Dark"}</span>
      </button>
      <button className="setting" data-focus data-fid="set:backdrop" onClick={onBackdrop}>
        <span>Game pictures behind the screen</span>
        <span className="setting__value">
          {backdrop === "full" ? "Full: the picture of the game in focus" : backdrop === "soft" ? "Soft: only its colours" : "Off: plain background"}
        </span>
      </button>
      <button className="setting" data-focus data-quiet data-fid="set:sounds" onClick={onSounds}>
        <span>Sounds</span>
        <span className="setting__value">{sounds ? "On" : "Off"}</span>
      </button>
      <button className="setting" data-focus data-fid="set:refresh" onClick={onRefresh}>
        <span>Refresh library</span>
        <span className="setting__value">{refreshing ? "Looking…" : `${gameCount} games`}</span>
      </button>
      <button className="setting" data-focus data-fid="set:downloads" onClick={onDownloads}>
        <span>Downloads</span>
        <span className="setting__value">{downloading ? `${downloading} being installed or updated` : "Nothing is being installed"}</span>
      </button>
      <button className="setting" data-focus data-fid="set:manage" onClick={onManage}>
        <span>Manage games</span>
        <span className="setting__value">Administrator only</span>
      </button>
      <button className="setting" data-focus data-fid="set:desktops" onClick={onDesktops}>
        <span>Desktops</span>
        <span className="setting__value">{hostCount ? `${hostCount} linked · ` : ""}Administrator only</span>
      </button>
      <button className="setting" data-focus data-fid="set:cleanup" onClick={onCleanup}>
        <span>Clean up Windows</span>
        <span className="setting__value">Administrator only</span>
      </button>
      <button className="setting" data-focus data-fid="set:updates" onClick={onUpdates}>
        <span>Updates</span>
        <span className="setting__value">{version ? `CarlaOS ${version} · ` : ""}Administrator only</span>
      </button>
      <button className="setting" data-focus data-fid="set:storage" onClick={onStorage}>
        <span>Storage</span>
        <span className="setting__value">Room for games, uninstall · Administrator only</span>
      </button>
      <button className="setting" data-focus data-fid="set:startup" onClick={onStartup}>
        <span>Starting up</span>
        <span className="setting__value">Open CarlaOS when the Ally turns on · Administrator only</span>
      </button>
      <button className="setting" data-focus data-fid="set:crash" onClick={onCrash}>
        <span>Crash reports</span>
        <span className="setting__value">Send a copy to whoever looks after CarlaOS</span>
      </button>
      <button className="setting" data-focus data-fid="set:log" onClick={onLog}>
        <span>Activity log</span>
        <span className="setting__value">What the connection to the desktops was doing</span>
      </button>
      <button className="setting" data-focus data-fid="set:exit" onClick={onExit}>
        <span>Exit to Windows</span>
        <span className="setting__value">Closes CarlaOS</span>
      </button>
      <p className="lede">CarlaOS {version}</p>
    </div>
  );
}

