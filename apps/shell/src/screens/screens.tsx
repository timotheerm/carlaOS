import { useState } from "react";
import type { CloudService, Game, Host } from "../data/types";
import { Cover, Empty, GameCard, Grid, HostCard, Row, placeLabel, playtime } from "../ui/parts";

export type Ctx = {
  games: Game[];
  hosts: Host[];
  favourites: Set<string>;
  openGame: (id: string) => void;
  openHost: (id: string) => void;
  notify: (text: string) => void;
  /** Start a game from one of the places it is installed. */
  play: (gameId: string, place: string) => void;
  /** Ask the desktops again how they are. */
  refreshHosts: () => void;
};

const byRecent = (a: Game, b: Game) => (b.lastPlayed ?? "").localeCompare(a.lastPlayed ?? "");
const countOn = (games: Game[], place: string) => games.filter((g) => g.places.includes(place)).length;

function cards(list: Game[], ctx: Ctx) {
  return list.map((g) => (
    <GameCard key={g.id} game={g} favourite={ctx.favourites.has(g.id)} onOpen={ctx.openGame} />
  ));
}

export function Home(ctx: Ctx) {
  const played = ctx.games.filter((g) => g.lastPlayed).sort(byRecent);
  const [hero, ...recent] = played;
  const favourites = ctx.games.filter((g) => ctx.favourites.has(g.id));
  const added = [...ctx.games].sort((a, b) => b.added.localeCompare(a.added)).slice(0, 8);

  if (ctx.games.length === 0) {
    return <Empty>No games yet. Install some, or add them under Settings, then Manage games.</Empty>;
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
          <Cover game={hero} wide />
          <span className="hero__text">
            <span className="hero__eyebrow">Continue playing</span>
            <span className="hero__title">{hero.title}</span>
            <span className="hero__meta">
              {hero.platform} · {playtime(hero.playtimeMinutes)}
            </span>
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
      {recent.length > 0 && <Row title="Recently played">{cards(recent.slice(0, 8), ctx)}</Row>}
      {favourites.length > 0 && <Row title="Favourites">{cards(favourites, ctx)}</Row>}
      <Row title="Recently added">{cards(added, ctx)}</Row>
    </>
  );
}

const FILTERS = [
  { id: "all", label: "All" },
  { id: "local", label: "On this Ally" },
  { id: "stream", label: "Streamable" },
  { id: "fav", label: "Favourites" },
] as const;

export function Games(ctx: Ctx) {
  const [filter, setFilter] = useState<(typeof FILTERS)[number]["id"]>("all");
  const list = ctx.games
    .filter((g) => g.kind === "game" || g.kind === "homebrew")
    .filter((g) =>
      filter === "local"
        ? g.places.includes("local")
        : filter === "stream"
          ? g.places.some((p) => p !== "local")
          : filter === "fav"
            ? ctx.favourites.has(g.id)
            : true,
    )
    .sort((a, b) => a.title.localeCompare(b.title));

  return (
    <>
      <div className="chips">
        {FILTERS.map((f, i) => (
          <button
            key={f.id}
            className={f.id === filter ? "chip chip--on" : "chip"}
            data-focus
            data-autofocus={i === 0 ? "" : undefined}
            data-fid={`filter:${f.id}`}
            onClick={() => setFilter(f.id)}
          >
            {f.label}
          </button>
        ))}
      </div>
      {list.length ? <Grid>{cards(list, ctx)}</Grid> : <Empty>Nothing here yet.</Empty>}
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
        {!awake && (
          <button className="pill pill--primary" data-focus data-autofocus data-fid="host:check" onClick={ctx.refreshHosts}>
            Check again
          </button>
        )}
      </div>
      {list.length ? <Grid>{cards(list, ctx)}</Grid> : <Empty>No games were found on this desktop yet.</Empty>}
    </>
  );
}

export function Emulation(ctx: Ctx) {
  const emulated = ctx.games.filter((g) => g.kind === "emulated");
  const consoles = [...new Set(emulated.map((g) => g.platform))].sort();
  if (emulated.length === 0) return <Empty>No emulated games yet. Emulators arrive in a later build.</Empty>;
  return (
    <>
      {consoles.map((c) => (
        <Row key={c} title={c}>
          {cards(emulated.filter((g) => g.platform === c), ctx)}
        </Row>
      ))}
    </>
  );
}

export function Ports(ctx: Ctx) {
  const ports = ctx.games.filter((g) => g.kind === "port");
  if (ports.length === 0) return <Empty>No native ports yet. Add one under Settings, then Manage games.</Empty>;
  return <Grid>{cards(ports, ctx)}</Grid>;
}

export function Cloud({ services, notify }: { services: CloudService[]; notify: Ctx["notify"] }) {
  return (
    <>
      <p className="lede">Rented cloud services. These are separate from your own desktops.</p>
      <div className="hosts">
        {services.map((s) => (
          <button
            key={s.id}
            className="host host--large host--cloud"
            style={{ "--hue": s.hue } as React.CSSProperties}
            data-focus
            data-fid={`cloud:${s.id}`}
            onClick={() => notify(`${s.name} opens from here in a later build.`)}
          >
            <span className="host__name">{s.name}</span>
            <span className="host__line host__line--quiet">{s.note}</span>
          </button>
        ))}
      </div>
    </>
  );
}

export function Settings({
  theme,
  onTheme,
  onCleanup,
  onManage,
  onDesktops,
  hostCount,
  onRefresh,
  refreshing,
  gameCount,
  onExit,
  version,
}: {
  onManage: () => void;
  onDesktops: () => void;
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
      <button className="setting" data-focus data-fid="set:refresh" onClick={onRefresh}>
        <span>Refresh library</span>
        <span className="setting__value">{refreshing ? "Looking…" : `${gameCount} games`}</span>
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
      <button className="setting" data-focus data-fid="set:exit" onClick={onExit}>
        <span>Exit to Windows</span>
        <span className="setting__value">Closes CarlaOS</span>
      </button>
      <p className="lede">CarlaOS {version}</p>
    </div>
  );
}

export function GameDetail({ gameId, onToggleFavourite, ...ctx }: Ctx & { gameId: string; onToggleFavourite: (id: string) => void }) {
  const game = ctx.games.find((g) => g.id === gameId);
  if (!game) return <Empty>This game is no longer in the library.</Empty>;
  const fav = ctx.favourites.has(game.id);
  return (
    <article className="detail" data-game={game.id}>
      <Cover game={game} wide />
      <div className="detail__body">
        <p className="hero__eyebrow">{game.platform}</p>
        <h1 className="detail__title">{game.title}</h1>
        <p className="detail__desc">{game.description}</p>
        <p className="detail__meta">{playtime(game.playtimeMinutes)}</p>
        <div className="detail__actions">
          {game.places.map((place, i) => {
            const host = ctx.hosts.find((h) => h.id === place);
            const away = host && host.state !== "ready" && host.state !== "busy" && host.state !== "unknown";
            const label = place === "local" ? "Play" : `Stream from ${placeLabel(place, ctx.hosts)}`;
            return (
              <button
                key={place}
                className={i === 0 ? "pill pill--primary" : "pill"}
                data-focus
                data-autofocus={i === 0 ? "" : undefined}
                data-fid={`play:${place}`}
                onClick={() => ctx.play(game.id, place)}
              >
                {label}
                {away ? (host.state === "not_ready" ? " (needs attention)" : " (not answering)") : ""}
              </button>
            );
          })}
          <button className="pill" data-focus data-fid="fav" onClick={() => onToggleFavourite(game.id)}>
            {fav ? "♥ Favourite" : "♡ Favourite"}
          </button>
        </div>
      </div>
    </article>
  );
}
