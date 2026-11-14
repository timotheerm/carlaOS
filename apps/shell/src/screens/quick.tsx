// The quick menu: what opens over a running game when View and Menu are held
// together. It lives in a window of its own, see-through around the panel, so
// the game stays visible behind it.
//
// The controller is read by the Rust side while a game is in front, and each
// step arrives here as an event. In a plain browser the keyboard stands in.
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { message } from "../library/store";
import { inTauri } from "../native";

type Sound = { level: number; muted: boolean };
type State = {
  title: string | null;
  stream: boolean;
  since: number | null;
  sound: Sound | null;
  brightness: number | null;
  battery: { percent: number; charging: boolean } | null;
};
type Press = "up" | "down" | "left" | "right" | "select" | "back";
type Row = "resume" | "sound" | "brightness" | "desktop" | "end";

const SAMPLE: State = {
  title: "Neon Harbor",
  stream: false,
  since: Math.floor(Date.now() / 1000) - 83 * 60,
  sound: { level: 60, muted: false },
  brightness: 70,
  battery: { percent: 64, charging: false },
};

const KEYS: Record<string, Press> = { ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right", Enter: "select", Escape: "back", Backspace: "back" };

// One notch up or down, the same way the Rust side would count it.
const step = (level: number, up: boolean) => (up ? Math.min(100, (Math.floor(level / 10) + 1) * 10) : Math.max(0, Math.floor((level - 1) / 10) * 10));

function played(since: number | null): string {
  if (since === null) return "";
  const minutes = Math.max(0, Math.floor((Date.now() / 1000 - since) / 60));
  if (minutes < 1) return "Just started";
  return minutes < 60 ? `Playing for ${minutes} min` : `Playing for ${Math.floor(minutes / 60)} h ${minutes % 60} min`;
}

function Level({ value, off = false }: { value: number; off?: boolean }) {
  return (
    <span className={off ? "quick__level quick__level--off" : "quick__level"} role="img" aria-label={`${value} percent`}>
      <i style={{ width: `${value}%` }} />
    </span>
  );
}

export function QuickMenu() {
  const [state, setState] = useState<State>(inTauri ? { title: null, stream: false, since: null, sound: null, brightness: null, battery: null } : SAMPLE);
  const [row, setRow] = useState<Row>("resume");
  const [confirming, setConfirming] = useState(false);
  const [sure, setSure] = useState(false); // which of the two answers is chosen
  const [note, setNote] = useState<string | null>(null);
  const [clock, setClock] = useState(() => new Date());

  const refresh = useCallback(() => {
    if (inTauri) invoke<State>("quick_state").then(setState, () => undefined);
  }, []);

  const close = useCallback(() => {
    setConfirming(false);
    if (inTauri) void invoke("quick_close");
  }, []);

  const apply = useCallback((call: () => Promise<State>, fallback: State) => {
    if (!inTauri) return setState(fallback);
    call().then(setState, (e) => setNote(message(e)));
  }, []);

  // Read through a ref so the one listener always sees the latest state.
  const act = useRef<(press: Press) => void>(() => undefined);
  act.current = (press) => {
    setNote(null);
    if (confirming) {
      if (press === "left" || press === "right") setSure((s) => !s);
      else if (press === "back" || (press === "select" && !sure)) setConfirming(false);
      else if (press === "select") {
        setConfirming(false);
        if (inTauri) void invoke("quick_end_game");
        else setNote("The game would close now.");
      }
      return;
    }
    const rows: Row[] = ["resume", ...(state.sound ? ["sound" as const] : []), ...(state.brightness !== null ? ["brightness" as const] : []), ...(state.stream ? ["desktop" as const] : []), "end"];
    const at = Math.max(0, rows.indexOf(row));
    if (press === "up") setRow(rows[Math.max(0, at - 1)]);
    else if (press === "down") setRow(rows[Math.min(rows.length - 1, at + 1)]);
    else if (press === "back") close();
    else if (row === "resume" && press === "select") close();
    else if (row === "desktop" && press === "select") {
      if (inTauri) invoke("quick_show_desktop").catch((e) => setNote(message(e)));
      else setNote("The desktop's loading screen would be taken away now.");
    } else if (row === "end" && press === "select") {
      setSure(false);
      setConfirming(true);
    } else if (row === "sound" && state.sound) {
      const sound = state.sound;
      // Changing the level also ends a mute, as on any remote control.
      const next = press === "select" ? { ...sound, muted: !sound.muted } : { level: step(sound.level, press === "right"), muted: false };
      apply(() => invoke<State>("quick_set_sound", next), { ...state, sound: next });
    } else if (row === "brightness" && state.brightness !== null && press !== "select") {
      const level = Math.max(5, step(state.brightness, press === "right"));
      apply(() => invoke<State>("quick_set_brightness", { level }), { ...state, brightness: level });
    }
  };

  useEffect(() => {
    document.documentElement.classList.add("quick-window");
    // The same light or dark look as the console it belongs to.
    try {
      document.documentElement.dataset.theme = JSON.parse(localStorage.getItem("carla.theme") ?? '"light"');
    } catch {
      document.documentElement.dataset.theme = "light";
    }
    const onKey = (e: KeyboardEvent) => {
      const press = KEYS[e.key];
      if (press) {
        e.preventDefault();
        act.current(press);
      }
    };
    window.addEventListener("keydown", onKey);
    const tick = setInterval(() => setClock(new Date()), 20_000);
    const stops: Promise<() => void>[] = [];
    if (inTauri) {
      stops.push(listen<Press>("quick://press", (e) => act.current(e.payload)));
      // Each time the menu is shown it starts at the top with fresh levels.
      stops.push(
        listen("quick://open", () => {
          setRow("resume");
          setConfirming(false);
          setNote(null);
          setClock(new Date());
          refresh();
        }),
      );
      refresh();
    }
    return () => {
      window.removeEventListener("keydown", onKey);
      clearInterval(tick);
      stops.forEach((stop) => void stop.then((s) => s()));
    };
  }, [refresh]);

  const cls = (r: Row) => (row === r && !confirming ? "quick__row quick__row--on" : "quick__row");
  const { sound, brightness, battery } = state;
  return (
    <div className={inTauri ? "quick" : "quick quick--preview"}>
      <section className="quick__panel" aria-label="Quick menu">
        <header className="quick__head">
          <div>
            <p className="hero__eyebrow">{state.stream ? "Streaming" : "Playing"}</p>
            <h1 className="quick__title">{state.title ?? "CarlaOS"}</h1>
            <p className="quick__sub">{played(state.since)}</p>
          </div>
          <div className="quick__status">
            <b>{clock.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</b>
            {battery && (
              <span className={battery.percent <= 15 && !battery.charging ? "quick__battery quick__battery--low" : "quick__battery"}>
                {battery.percent}%{battery.charging ? " ⚡" : ""}
              </span>
            )}
          </div>
        </header>

        <div className={cls("resume")} onClick={close}>
          <span className="quick__name">Back to the game</span>
        </div>
        {sound && (
          <div className={cls("sound")}>
            <span className="quick__name">Sound</span>
            <Level value={sound.level} off={sound.muted} />
            <span className="quick__value">{sound.muted ? "Muted" : `${sound.level}%`}</span>
          </div>
        )}
        {brightness !== null && (
          <div className={cls("brightness")}>
            <span className="quick__name">Brightness</span>
            <Level value={brightness} />
            <span className="quick__value">{brightness}%</span>
          </div>
        )}
        {state.stream && (
          <div className={cls("desktop")}>
            <span className="quick__name">Show the desktop</span>
            <span className="quick__value">when the game is waiting for an answer there</span>
          </div>
        )}
        <div className={row === "end" || confirming ? "quick__row quick__row--on quick__row--end" : "quick__row quick__row--end"}>
          {confirming ? (
            <>
              <span className="quick__question">End the game? Anything not saved is lost.</span>
              <span className="quick__answers">
                <span className={sure ? "quick__answer" : "quick__answer quick__answer--on"}>Keep playing</span>
                <span className={sure ? "quick__answer quick__answer--on quick__answer--end" : "quick__answer"}>End game</span>
              </span>
            </>
          ) : (
            <span className="quick__name">{state.stream ? "End game and stream" : "End game"}</span>
          )}
        </div>

        <footer className="quick__hints">
          {note ? (
            <span className="quick__note">{note}</span>
          ) : (
            <>
              <span><kbd>A</kbd> {row === "sound" && !confirming ? "Mute" : "Select"}</span>
              <span><kbd>B</kbd> Back</span>
              {(row === "sound" || row === "brightness" || confirming) && <span><kbd>◀ ▶</kbd> Change</span>}
              <span className="quick__how">Hold View + Menu to close</span>
            </>
          )}
        </footer>
      </section>
    </div>
  );
}
