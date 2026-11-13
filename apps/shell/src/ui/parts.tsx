import type { ReactNode } from "react";
import type { Game, Host, HostState } from "../data/types";
import { Heart, OnDevice, Source, Streamed } from "./icons";

export function playtime(minutes: number): string {
  if (minutes === 0) return "Not played yet";
  if (minutes < 60) return `${minutes} min played`;
  const hours = Math.round(minutes / 60);
  return `${hours} h played`;
}

/** "12 h", "45 min", "0 min": the bare amount, for a tile with its own label. */
export function amount(minutes: number): string {
  return minutes < 60 ? `${minutes} min` : `${Math.round(minutes / 60)} h`;
}

/** "today, 21:05", "yesterday, 22:14", "3 March". */
export function when(seconds: number | null): string {
  if (seconds === null) return "never";
  const then = new Date(seconds * 1000);
  const days = Math.round((new Date().setHours(0, 0, 0, 0) - new Date(then).setHours(0, 0, 0, 0)) / 86_400_000);
  const time = then.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  if (days <= 0) return `today, ${time}`;
  if (days === 1) return `yesterday, ${time}`;
  return then.toLocaleDateString([], { day: "numeric", month: "long" });
}

/** "Today", "Yesterday", "10 Nov": short enough for a tile. */
export function day(seconds: number | null): string {
  if (seconds === null) return "Never";
  const then = new Date(seconds * 1000);
  const days = Math.round((new Date().setHours(0, 0, 0, 0) - new Date(then).setHours(0, 0, 0, 0)) / 86_400_000);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  return then.toLocaleDateString([], { day: "numeric", month: "short" });
}

/** "2.6 MB", "340 KB". */
export function size(bytes: number): string {
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1000))} KB`;
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

/** Where a game is from, and whether it plays offline or streams. */
export function Badges({ game, hosts }: { game: Game; hosts?: Host[] }) {
  const local = game.places.includes("local");
  const remote = game.places.filter((p) => p !== "local");
  const from = remote.length === 1 && hosts ? placeLabel(remote[0], hosts) : "a desktop";
  return (
    <span className="badges">
      <span className="badge" title={`From ${game.platform}`}>
        <Source kind={game.kind} />
        {game.platform}
      </span>
      {local && (
        <span className="badge badge--offline" title="Installed on this Ally. Plays without a desktop.">
          <OnDevice />
          Offline
        </span>
      )}
      {remote.length > 0 && (
        <span className="badge badge--stream" title={`Streams from ${from}. Needs a connection to it.`}>
          <Streamed />
          {local ? "" : "Stream"}
        </span>
      )}
    </span>
  );
}

/** The same in words, for screen readers. */
export function availability(game: Game): string {
  const local = game.places.includes("local");
  const remote = game.places.some((p) => p !== "local");
  return local && remote ? "plays offline, can also stream" : local ? "plays offline" : "streams from a desktop";
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
      aria-label={`${game.title}, from ${game.platform}, ${availability(game)}${favourite ? ", favourite" : ""}`}
    >
      <Cover game={game} />
      <Badges game={game} />
      {favourite && (
        <span className="card__heart">
          <Heart />
        </span>
      )}
      <span className="card__meta">{playtime(game.playtimeMinutes)}</span>
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
