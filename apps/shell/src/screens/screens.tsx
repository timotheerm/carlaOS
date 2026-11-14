import { useLayoutEffect, useState } from "react";
import { focusFirst } from "../nav/focus";
import type { CloudService, Game, Host } from "../data/types";
import { Cover, Empty, GameCard, Grid, HostCard, Row, playtime } from "../ui/parts";

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
  /** Wake a sleeping desktop. */
  wakeHost: (id: string) => void;
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
  { id: "stream", label: "Streamed" },
  { id: "fav", label: "Favourites" },
] as const;

export function Games(ctx: Ctx) {
  const [filter, setFilter] = useState<(typeof FILTERS)[number]["id"]>("all");
  const list = ctx.games
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

export function Cloud({ services, notify }: { services: CloudService[]; notify: Ctx["notify"] }) {
  return (
    <>
      <p className="lede">Cloud gaming: games that run in a rented data centre. Everything else is under My Games.</p>
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
            <span className="cloud__kind">{s.kind}</span>
            <span className="host__name">{s.name}</span>
            <span className="host__line host__line--quiet">{s.note}</span>
            <span className="cloud__glow" aria-hidden />
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
  onUpdates,
  onStartup,
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
      <button className="setting" data-focus data-quiet data-fid="set:sounds" onClick={onSounds}>
        <span>Sounds</span>
        <span className="setting__value">{sounds ? "On" : "Off"}</span>
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
      <button className="setting" data-focus data-fid="set:updates" onClick={onUpdates}>
        <span>Updates</span>
        <span className="setting__value">{version ? `CarlaOS ${version} · ` : ""}Administrator only</span>
      </button>
      <button className="setting" data-focus data-fid="set:startup" onClick={onStartup}>
        <span>Starting up</span>
        <span className="setting__value">Open CarlaOS when the Ally turns on · Administrator only</span>
      </button>
      <button className="setting" data-focus data-fid="set:exit" onClick={onExit}>
        <span>Exit to Windows</span>
        <span className="setting__value">Closes CarlaOS</span>
      </button>
      <p className="lede">CarlaOS {version}</p>
    </div>
  );
}

