import { useEffect, useState } from "react";

export const SECTIONS = [
  { id: "home", label: "Home" },
  { id: "games", label: "My Games" },
  { id: "desktops", label: "My Desktops" },
  { id: "emulation", label: "Emulation" },
  { id: "ports", label: "Native Ports" },
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

export function TopBar({ section, onSelect }: { section: Section; onSelect: (s: Section) => void }) {
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

export function Hints({ items }: { items: [string, string][] }) {
  return (
    <footer className="hints" aria-hidden>
      {items.map(([key, label]) => (
        <span key={key} className="hint">
          <b>{key}</b>
          {label}
        </span>
      ))}
    </footer>
  );
}
