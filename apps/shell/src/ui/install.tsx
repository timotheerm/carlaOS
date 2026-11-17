// Where a game stands: installed, not installed, or somewhere in between.
// One answer for the whole interface, so a card, the game's page and the
// downloads page never say different things about the same game.
import type { Download, Game, Host } from "../data/types";

export type StateKey = "installed" | "stream" | "cloud" | "absent" | "queued" | "installing" | "paused" | "finishing" | "update" | "updating";

export type InstallState = {
  key: StateKey;
  /** A few words: "Installing · 42 %". */
  label: string;
  /** 0 to 100 while something is being fetched and it is known how far. */
  percent: number | null;
  /** "About 12 min left", when it can be told. */
  left: string | null;
  tone: "ok" | "busy" | "wait" | "off" | "warn";
};

export function percentOf(d: { done: number; total: number; phase?: string }): number | null {
  if (d.phase === "finishing") return 100;
  return d.total > 0 ? Math.min(100, Math.floor((d.done / d.total) * 100)) : null;
}

/** "About 12 min left", "Under a minute left". */
export function timeLeft(d: Download): string | null {
  if (d.phase !== "downloading" || d.eta === null || d.eta === undefined) return null;
  if (d.eta < 60) return "Under a minute left";
  if (d.eta < 3600) return `About ${Math.round(d.eta / 60)} min left`;
  const hours = Math.floor(d.eta / 3600);
  const minutes = Math.round((d.eta % 3600) / 600) * 10;
  return `About ${hours} h${minutes ? ` ${minutes} min` : ""} left`;
}

/** "24.5 MB/s". */
export function speed(d: Download): string | null {
  if (d.phase !== "downloading" || !d.rate) return null;
  return d.rate >= 1_000_000 ? `${(d.rate / 1_000_000).toFixed(1)} MB/s` : `${Math.max(1, Math.round(d.rate / 1000))} KB/s`;
}

/** What one download is doing, in words. */
export function stateOfDownload(d: Download): InstallState {
  const percent = percentOf(d);
  const what = d.update ? "Updating" : "Installing";
  if (d.phase === "finishing") return { key: "finishing", label: "Finishing up", percent: 100, left: null, tone: "busy" };
  if (d.phase === "paused") return { key: "paused", label: percent === null ? "Paused" : `Paused at ${percent} %`, percent, left: null, tone: "wait" };
  if (d.phase === "queued" || percent === null) return { key: "queued", label: d.update ? "Update queued" : "Queued", percent: null, left: null, tone: "wait" };
  return { key: d.update ? "updating" : "installing", label: `${what} · ${percent} %`, percent, left: timeLeft(d), tone: "busy" };
}

/** The download that says most about a game: the one on this Ally first. */
export function downloadOf(gameId: string, downloads: Download[]): Download | undefined {
  const own = downloads.filter((d) => d.gameId === gameId);
  return own.find((d) => d.place === "local") ?? own[0];
}

/** Where a game stands as a whole, for its card. */
export function installState(game: Game, downloads: Download[]): InstallState {
  const download = downloadOf(game.id, downloads);
  if (download) return stateOfDownload(download);
  const waiting = game.updates ?? [];
  const running = waiting.find((u) => u.state === "running");
  if (running) {
    const percent = percentOf({ done: running.done, total: running.bytes });
    return { key: "updating", label: percent === null ? "Updating" : `Updating · ${percent} %`, percent, left: null, tone: "busy" };
  }
  if (waiting.length > 0) return { key: "update", label: waiting[0].state === "paused" ? "Update paused" : "Update needed", percent: null, left: null, tone: "warn" };
  if (game.places.includes("local")) return { key: "installed", label: "Installed", percent: null, left: null, tone: "ok" };
  if (game.places.includes("cloud")) return { key: "cloud", label: "Ready in the cloud", percent: null, left: null, tone: "ok" };
  if (game.places.length > 0) return { key: "stream", label: "Ready to stream", percent: null, left: null, tone: "ok" };
  return { key: "absent", label: "Not installed", percent: null, left: null, tone: "off" };
}

/** Where a game stands in one place: this Ally, the cloud or one desktop. */
export function stateAt(game: Game, place: string, downloads: Download[]): InstallState {
  const download = downloads.find((d) => d.gameId === game.id && d.place === place);
  if (download) return stateOfDownload(download);
  const update = (game.updates ?? []).find((u) => u.place === place);
  if (update?.state === "running") {
    const percent = percentOf({ done: update.done, total: update.bytes });
    return { key: "updating", label: percent === null ? "Updating" : `Updating · ${percent} %`, percent, left: null, tone: "busy" };
  }
  if (update) return { key: "update", label: update.state === "paused" ? "Update paused" : "Update needed", percent: null, left: null, tone: "warn" };
  if (game.places.includes(place)) {
    if (place === "local") return { key: "installed", label: "Installed", percent: null, left: null, tone: "ok" };
    if (place === "cloud") return { key: "cloud", label: "Ready in the cloud", percent: null, left: null, tone: "ok" };
    return { key: "stream", label: "Installed · ready to stream", percent: null, left: null, tone: "ok" };
  }
  return { key: "absent", label: "Not installed", percent: null, left: null, tone: "off" };
}

/** A dot and a few words: the state of a game, the same everywhere. */
export function StateLine({ state, extra }: { state: InstallState; extra?: string }) {
  return (
    <span className={`stateline stateline--${state.tone}`}>
      <i aria-hidden />
      <span>
        {state.label}
        {extra ? ` · ${extra}` : ""}
      </span>
    </span>
  );
}

/** A ring that fills as a game is fetched, with the number in the middle. */
export function Ring({ state }: { state: InstallState }) {
  const busy = state.key === "queued" || state.key === "finishing";
  const part = state.percent ?? 0;
  return (
    <span className={`ring${busy ? " ring--turning" : ""}${state.key === "paused" ? " ring--paused" : ""}`} aria-hidden>
      <svg viewBox="0 0 100 100">
        <circle className="ring__track" cx="50" cy="50" r="42" />
        <circle className="ring__fill" cx="50" cy="50" r="42" pathLength={100} strokeDasharray={`${busy ? 28 : Math.max(part, 1)} 100`} />
      </svg>
      <span className="ring__text">
        {state.key === "queued" ? "Queued" : state.key === "finishing" ? "Finishing" : state.key === "paused" ? "Paused" : (
          <>
            {part}
            <small>%</small>
          </>
        )}
      </span>
    </span>
  );
}

const STEPS = ["Queued", "Downloading", "Finishing", "Ready"] as const;

/** The four steps of an install, with the one it is at. */
export function Steps({ download }: { download: Download }) {
  const at = download.phase === "queued" || !download.total ? 0 : download.phase === "finishing" ? 2 : 1;
  return (
    <ol className="steps" aria-label={`Step ${at + 1} of 4: ${STEPS[at]}`}>
      {STEPS.map((name, i) => (
        <li key={name} className={i < at ? "steps__step steps__step--done" : i === at ? "steps__step steps__step--now" : "steps__step"} aria-current={i === at ? "step" : undefined}>
          <i aria-hidden>{i < at ? "✓" : i + 1}</i>
          {name === "Downloading" && download.phase === "paused" ? "Paused" : name}
        </li>
      ))}
    </ol>
  );
}

/** Where a place is, in words. */
export function placeName(place: string, hosts: Host[]): string {
  if (place === "local") return "This Ally";
  if (place === "cloud") return "The cloud";
  return hosts.find((h) => h.id === place)?.name ?? "A desktop";
}
