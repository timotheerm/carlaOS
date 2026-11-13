import type { ReactNode } from "react";
import type { Download, Game, Host, HostState } from "../data/types";
import { Heart, OnDevice, Source, Streamed } from "./icons";
import { banner } from "./platform";

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

// The console a game is from, as a band along the bottom of its picture.
function Band({ game }: { game: Game }) {
  const band = banner(game);
  if (!band) return null;
  return (
    <span className="cover__band" style={{ background: band.colour, color: band.ink }}>
      {band.label}
    </span>
  );
}

/** How far a download is, in a few words. */
export function fetching(d: Download): string {
  const part = d.total ? Math.min(100, Math.floor((d.done / d.total) * 100)) : 0;
  if (d.phase === "finishing") return "Almost ready";
  if (d.phase === "paused") return `Paused at ${part} %`;
  if (d.phase === "queued" || !d.total) return "Waiting for Steam";
  return `${part} %`;
}

/** The pink bar of a download. */
export function Progress({ download }: { download: Download }) {
  const part = download.phase === "finishing" ? 100 : download.total ? Math.min(100, (download.done / download.total) * 100) : 0;
  const waiting = download.phase === "queued" || !download.total;
  return (
    <span
      className={`progress progress--${waiting ? "waiting" : download.phase}`}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(part)}
      aria-label={`${download.title}: ${fetching(download)}`}
    >
      <i style={{ width: `${waiting ? 100 : Math.max(part, 2)}%` }} />
    </span>
  );
}

// A game's picture. Without real artwork: a soft gradient from the game's
// hue with its name on it.
export function Cover({ game, wide = false, band = true }: { game: Game; wide?: boolean; band?: boolean }) {
  const picture = wide ? (game.hero ?? game.cover) : (game.cover ?? game.hero);
  const banded = band && banner(game) !== null;
  const shape = `${wide ? "cover cover--wide" : "cover"}${banded ? " cover--banded" : ""}`;
  if (picture) {
    return (
      <div className={`${shape} cover--art`} aria-hidden>
        {/* A tall cover in a wide frame sits whole on a soft copy of itself. */}
        {wide && !game.hero && <img className="cover__behind" src={picture} alt="" draggable={false} />}
        <img className={wide && !game.hero ? "cover__whole" : undefined} src={picture} alt="" loading="lazy" draggable={false} />
        {band && <Band game={game} />}
      </div>
    );
  }
  return (
    <div className={shape} style={{ "--hue": game.hue } as React.CSSProperties} aria-hidden>
      <span className="cover__title">{game.title}</span>
      {band && <Band game={game} />}
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
        {game.kind === "port" && game.platform === "Standalone" ? "Port" : game.platform}
      </span>
      {game.places.length === 0 && (
        <span className="badge badge--absent" title="In the library, not installed yet.">
          Not installed
        </span>
      )}
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
  if (!local && !remote) return "not installed yet";
  return local && remote ? "plays offline, can also stream" : local ? "plays offline" : "streams from a desktop";
}

export function GameCard({
  game,
  favourite,
  onOpen,
  download,
}: {
  game: Game;
  favourite: boolean;
  onOpen: (id: string) => void;
  /** What its store is fetching for it, if anything. */
  download?: Download;
}) {
  const absent = game.places.length === 0;
  return (
    <button
      className={absent ? "card card--absent" : "card"}
      data-focus
      data-fid={`game:${game.id}`}
      data-game={game.id}
      onClick={() => onOpen(game.id)}
      aria-label={`${game.title}, from ${game.platform}, ${download ? `installing, ${fetching(download)}` : availability(game)}${favourite ? ", favourite" : ""}`}
    >
      <Cover game={game} />
      <Badges game={game} />
      {favourite && (
        <span className="card__heart">
          <Heart />
        </span>
      )}
      {download && <Progress download={download} />}
      <span className="card__meta">
        {download ? `${download.update ? "Updating" : "Installing"} · ${fetching(download)}` : absent ? "Ready to install" : playtime(game.playtimeMinutes)}
      </span>
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
