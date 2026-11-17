import { useEffect, useRef, useState, type ReactNode } from "react";
import type { Download, Game } from "../data/types";
import { StateLine, installState, percentOf } from "./install";
import { Download as DownloadIcon } from "./icons";
import { playtime } from "./parts";

export const SECTIONS = [
  { id: "home", label: "Home" },
  { id: "games", label: "My Games" },
  { id: "desktops", label: "My Desktops" },
  { id: "cloud", label: "Cloud" },
  { id: "settings", label: "Settings" },
] as const;

export type Section = (typeof SECTIONS)[number]["id"];

type BatteryLike = { level: number; charging: boolean; addEventListener: (t: string, f: () => void) => void };

function useStatus() {
  const [now, setNow] = useState(() => new Date());
  const [battery, setBattery] = useState<{ level: number; charging: boolean } | null>(null);
  const [online, setOnline] = useState(navigator.onLine);

  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 15_000);
    const on = () => setOnline(navigator.onLine);
    window.addEventListener("online", on);
    window.addEventListener("offline", on);
    const nav = navigator as Navigator & { getBattery?: () => Promise<BatteryLike> };
    nav.getBattery?.().then((b) => {
      const read = () => setBattery({ level: b.level, charging: b.charging });
      read();
      b.addEventListener("levelchange", read);
      b.addEventListener("chargingchange", read);
    }).catch(() => undefined);
    return () => {
      clearInterval(id);
      window.removeEventListener("online", on);
      window.removeEventListener("offline", on);
    };
  }, []);

  return { now, battery, online };
}

export function TopBar({ section, onSelect, downloads = [], onDownloads }: { section: Section; onSelect: (s: Section) => void; downloads?: Download[]; onDownloads?: () => void }) {
  // One number for everything being fetched: how far it is all together.
  const known = downloads.filter((d) => d.total > 0);
  const overall = known.length ? percentOf({ done: known.reduce((n, d) => n + d.done, 0), total: known.reduce((n, d) => n + d.total, 0) }) : null;
  const { now, battery, online } = useStatus();
  return (
    <header className="top">
      <span className="top__hint" aria-hidden>LB</span>
      <nav className="rail" aria-label="Sections">
        {SECTIONS.map((s) => (
          <button
            key={s.id}
            className={s.id === section ? "rail__item rail__item--on" : "rail__item"}
            data-focus
            data-fid={`rail:${s.id}`}
            aria-current={s.id === section ? "page" : undefined}
            onClick={() => onSelect(s.id)}
          >
            {s.label}
          </button>
        ))}
      </nav>
      <span className="top__hint" aria-hidden>RB</span>
      <div className="status">
        {downloads.length > 0 && (
          <button className="top__downloads" data-focus data-fid="top:downloads" onClick={onDownloads} aria-label={`Downloads: ${downloads.length} being installed${overall === null ? "" : `, ${overall} % done`}`}>
            <DownloadIcon />
            {downloads.length}
            {overall === null ? "" : ` · ${overall} %`}
            <i style={{ width: `${overall ?? 0}%` }} />
          </button>
        )}
        <span>{online ? "Online" : "Offline"}</span>
        {battery && (
          <span>
            {Math.round(battery.level * 100)}%{battery.charging ? " ⚡" : ""}
          </span>
        )}
        <span className="status__clock">
          {now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
        </span>
      </div>
    </header>
  );
}

/** The game in focus, in one line at the foot of the screen: its name and
 *  how it stands, whichever card or button of it focus is on. */
export function FocusLine({ games, downloads }: { games: Game[]; downloads: Download[] }) {
  const [id, setId] = useState<string | null>(null);
  const timer = useRef(0);
  useEffect(() => {
    const onFocus = (e: FocusEvent) => {
      const found = (e.target as HTMLElement | null)?.closest?.<HTMLElement>("[data-game]")?.dataset.game ?? null;
      clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setId(found), 90);
    };
    document.addEventListener("focusin", onFocus);
    return () => {
      clearTimeout(timer.current);
      document.removeEventListener("focusin", onFocus);
    };
  }, []);
  const game = id ? games.find((g) => g.id === id) : undefined;
  if (!game) return <span className="focusline" />;
  return (
    <span className="focusline" aria-hidden>
      <b>{game.title}</b>
      <StateLine state={installState(game, downloads)} extra={game.places.length ? playtime(game.playtimeMinutes) : game.platform} />
    </span>
  );
}

export function Hints({ items, left }: { items: [string, string][]; left?: ReactNode }) {
  return (
    <footer className="hints" aria-hidden>
      {left}
      <span className="hints__keys">
        {items.map(([key, label]) => (
          <span key={key} className="hint">
            <b>{key}</b>
            {label}
          </span>
        ))}
      </span>
    </footer>
  );
}
