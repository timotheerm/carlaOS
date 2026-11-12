import type { ReactNode } from "react";
import type { Game, Host, HostState } from "../data/types";

export function playtime(minutes: number): string {
  if (minutes === 0) return "Not played yet";
  if (minutes < 60) return `${minutes} min played`;
  const hours = Math.round(minutes / 60);
  return `${hours} h played`;
}

export function placeLabel(place: string, hosts: Host[]): string {
  if (place === "local") return "On this Ally";
  return hosts.find((h) => h.id === place)?.name ?? place;
}

// Placeholder artwork: a soft gradient from the game's hue. Real covers replace
// this in the library milestone.
export function Cover({ game, wide = false }: { game: Game; wide?: boolean }) {
  const picture = wide ? (game.hero ?? game.cover) : game.cover;
  if (picture) {
    return (
      <div className={wide ? "cover cover--wide cover--art" : "cover cover--art"} aria-hidden>
        <img src={picture} alt="" loading="lazy" draggable={false} />
      </div>
    );
  }
  return (
    <div
      className={wide ? "cover cover--wide" : "cover"}
      style={{ "--hue": game.hue } as React.CSSProperties}
      aria-hidden
    >
      <span className="cover__title">{game.title}</span>
    </div>
  );
}

function Badge({ game }: { game: Game }) {
  const local = game.places.includes("local");
  const remote = game.places.some((p) => p !== "local");
  const text = local && remote ? "Ally + stream" : local ? "On Ally" : "Stream";
  return <span className="badge">{text}</span>;
}

export function GameCard({
  game,
  favourite,
  onOpen,
}: {
  game: Game;
  favourite: boolean;
  onOpen: (id: string) => void;
}) {
  return (
    <button
      className="card"
      data-focus
      data-fid={`game:${game.id}`}
      data-game={game.id}
      onClick={() => onOpen(game.id)}
      aria-label={`${game.title}, ${game.platform}`}
    >
      <Cover game={game} />
      <Badge game={game} />
      {favourite && (
        <span className="card__heart" aria-label="Favourite">
          ♥
        </span>
      )}
      <span className="card__meta">{game.platform}</span>
    </button>
  );
}

export function Row({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="row">
      <h2 className="row__title">{title}</h2>
      <div className="row__track">{children}</div>
    </section>
  );
}

const STATE_TEXT: Record<HostState, string> = {
  ready: "Ready",
  busy: "In use",
  not_ready: "Needs attention",
  asleep: "Asleep",
  off: "Off",
  unreachable: "Not answering",
  unknown: "Checking",
};

const STATE_TONE: Record<HostState, "ok" | "warn" | "off"> = {
  ready: "ok",
  busy: "warn",
  not_ready: "warn",
  asleep: "off",
  off: "off",
  unreachable: "off",
  unknown: "off",
};

export function HostCard({
  host,
  gameCount,
  onOpen,
  large = false,
}: {
  host: Host;
  gameCount: number;
  onOpen: (id: string) => void;
  large?: boolean;
}) {
  const route =
    host.route === "local" ? "Home network" : host.route === "remote" ? "Remote" : host.route === "relayed" ? "Relayed" : null;
  return (
    <button
      className={large ? "host host--large" : "host"}
      data-focus
      data-fid={`host:${host.id}`}
      onClick={() => onOpen(host.id)}
    >
      <span className="host__top">
        <span className="host__name">{host.name}</span>
        <span className={`state state--${STATE_TONE[host.state]}`}>
          <i />
          {STATE_TEXT[host.state]}
        </span>
      </span>
      <span className="host__line">
        {gameCount} games
        {route ? ` · ${route}` : ""}
        {host.latencyMs !== null ? ` · ${host.latencyMs} ms` : ""}
      </span>
      {large && <span className="host__line host__line--quiet">{host.hardware}</span>}
      {large && host.state === "busy" && host.playing && <span className="host__line host__line--quiet">Playing {host.playing}</span>}
      {large && host.state !== "ready" && host.state !== "busy" && <span className="host__line host__line--quiet">Last seen {host.lastSeen}</span>}
    </button>
  );
}

export function Grid({ children }: { children: ReactNode }) {
  return <div className="grid">{children}</div>;
}

export function Empty({ children }: { children: ReactNode }) {
  return <p className="empty">{children}</p>;
}
