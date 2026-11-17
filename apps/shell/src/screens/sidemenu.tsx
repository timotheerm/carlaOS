// The side menu, opened with the Menu button anywhere in the library: sound,
// brightness, the Wi-Fi network, the battery, and sleeping, restarting or
// turning off the Ally. Everything is a normal focusable button, so a
// controller and the keyboard both drive it. A power action waits for one more
// press, so nothing is switched off by accident.
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../native";
import { message } from "../library/store";

type System = {
  sound: { level: number; muted: boolean } | null;
  brightness: number | null;
  battery: { percent: number; charging: boolean } | null;
  wifi: { name: string; signal: number } | null;
};
type Power = "sleep" | "restart" | "off";

const SAMPLE: System = { sound: { level: 60, muted: false }, brightness: 70, battery: { percent: 64, charging: false }, wifi: { name: "Maison Rose", signal: 86 } };
const POWER_NAME: Record<Power, string> = { sleep: "Sleep", restart: "Restart", off: "Turn off" };

const step = (level: number, up: boolean) => (up ? Math.min(100, (Math.floor(level / 10) + 1) * 10) : Math.max(0, Math.floor((level - 1) / 10) * 10));

export function SideMenu({ onClose, notify }: { onClose: () => void; notify: (text: string) => void }) {
  const [system, setSystem] = useState<System>(inTauri ? { sound: null, brightness: null, battery: null, wifi: null } : SAMPLE);
  const [asking, setAsking] = useState<Power | null>(null);
  const panel = useRef<HTMLElement>(null);

  const reload = useCallback(() => {
    if (inTauri) invoke<System>("system_state").then(setSystem, () => undefined);
  }, []);

  useEffect(() => {
    reload();
  }, [reload]);

  const sendSound = (next: NonNullable<System["sound"]>) => {
    setSystem((s) => ({ ...s, sound: next }));
    if (inTauri) invoke<System>("system_set_sound", next).then(setSystem, (e) => notify(message(e)));
  };
  const sendBrightness = (level: number) => {
    setSystem((s) => ({ ...s, brightness: level }));
    if (inTauri) invoke<System>("system_set_brightness", { level }).then(setSystem, (e) => notify(message(e)));
  };
  const power = (p: Power) => {
    setAsking(null);
    if (!inTauri) return notify(`${POWER_NAME[p]} would happen now.`);
    invoke("system_power", { action: p }).catch((e) => notify(message(e)));
  };

  // Left and right change the level of the row in focus; the keyboard is the
  // only one that needs this, a controller uses the − and + buttons.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
      const row = (document.activeElement as HTMLElement | null)?.dataset.level;
      if (!row) return;
      e.preventDefault();
      e.stopPropagation();
      const up = e.key === "ArrowRight";
      if (row === "sound" && system.sound) sendSound({ level: step(system.sound.level, up), muted: false });
      if (row === "brightness" && system.brightness !== null) sendBrightness(Math.max(5, step(system.brightness, up)));
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  useEffect(() => {
    panel.current?.querySelector<HTMLElement>("[data-focus]")?.focus();
  }, []);

  const { sound, brightness, battery, wifi } = system;
  return (
    <div className="side" role="dialog" aria-label="Quick settings" onClick={onClose}>
      <section className="side__panel" ref={panel} onClick={(e) => e.stopPropagation()}>
        <h2 className="side__title">Quick</h2>
        {sound && (
          <div className="side__row">
            <button className="side__name" data-focus data-level="sound" onClick={() => sendSound({ ...sound, muted: !sound.muted })}>
              {sound.muted ? "Sound: muted" : "Sound"}
            </button>
            <button data-focus onClick={() => sendSound({ level: step(sound.level, false), muted: false })} aria-label="Quieter">−</button>
            <span className="side__value">{sound.muted ? "Muted" : `${sound.level}%`}</span>
            <button data-focus onClick={() => sendSound({ level: step(sound.level, true), muted: false })} aria-label="Louder">+</button>
          </div>
        )}
        {brightness !== null && (
          <div className="side__row">
            <button className="side__name" data-focus data-level="brightness" onClick={() => undefined}>Brightness</button>
            <button data-focus onClick={() => sendBrightness(Math.max(5, step(brightness, false)))} aria-label="Dimmer">−</button>
            <span className="side__value">{brightness}%</span>
            <button data-focus onClick={() => sendBrightness(step(brightness, true))} aria-label="Brighter">+</button>
          </div>
        )}
        {wifi && (
          <div className="side__row" aria-label={`Wi-Fi ${wifi.name}, ${wifi.signal} percent`}>
            <span className="side__name">{wifi.name}</span>
            <span className="side__value">Wi-Fi {wifi.signal}%</span>
          </div>
        )}
        {battery && (
          <div className="side__row">
            <span className="side__name">Battery</span>
            <span className="side__value">{battery.percent}%{battery.charging ? " ⚡" : ""}</span>
          </div>
        )}
        {(["sleep", "restart", "off"] as Power[]).map((p) => (
          <button key={p} className="side__row" data-focus onClick={() => (asking === p ? power(p) : setAsking(p))}>
            <span className={p === "off" ? "side__name side__danger" : "side__name"}>{asking === p ? `${POWER_NAME[p]}? Press again` : POWER_NAME[p]}</span>
          </button>
        ))}
        <p className="side__hint">Left and right change a level. B closes this.</p>
      </section>
    </div>
  );
}
