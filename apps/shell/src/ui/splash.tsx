// The startup animation: a heart draws itself, sparkles, and the name rises.
// It never holds anyone up: any key, button or touch skips it, and it leaves
// by itself after a moment.
import { useEffect, useState } from "react";

const SHOWN_MS = 2600;
const LEAVING_MS = 450;

const SPARKS = [
  { x: 18, y: 26, s: 1.1, d: 0.5 },
  { x: 80, y: 20, s: 0.8, d: 0.8 },
  { x: 70, y: 72, s: 1.3, d: 1.1 },
  { x: 26, y: 70, s: 0.7, d: 1.3 },
  { x: 50, y: 12, s: 0.6, d: 1.0 },
  { x: 90, y: 50, s: 0.9, d: 1.5 },
  { x: 9, y: 48, s: 0.9, d: 0.9 },
];

export function Splash({ onDone }: { onDone: () => void }) {
  const [leaving, setLeaving] = useState(false);

  useEffect(() => {
    const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const leave = () => setLeaving(true);
    // Nothing underneath has focus meanwhile, so a controller's A presses nothing.
    (document.activeElement as HTMLElement | null)?.blur();
    const timer = setTimeout(leave, still ? 900 : SHOWN_MS);
    // The press that skips must not also press what is underneath.
    const swallow = (e: Event) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      leave();
    };
    window.addEventListener("keydown", swallow, true);
    window.addEventListener("pointerdown", leave);
    // Any controller button skips too.
    let raf = 0;
    const poll = () => {
      raf = requestAnimationFrame(poll);
      const pad = navigator.getGamepads?.().find((p) => p && p.connected);
      if (pad?.buttons.some((b) => b.pressed)) leave();
    };
    raf = requestAnimationFrame(poll);
    return () => {
      clearTimeout(timer);
      cancelAnimationFrame(raf);
      window.removeEventListener("keydown", swallow, true);
      window.removeEventListener("pointerdown", leave);
    };
  }, []);

  useEffect(() => {
    if (!leaving) return;
    const timer = setTimeout(onDone, LEAVING_MS);
    return () => clearTimeout(timer);
  }, [leaving, onDone]);

  return (
    <div className={leaving ? "splash splash--leaving" : "splash"} role="img" aria-label="CarlaOS is starting">
      <div className="splash__glow" />
      {SPARKS.map((p, i) => (
        <svg
          key={i}
          className="splash__spark"
          viewBox="0 0 24 24"
          style={{ left: `${p.x}%`, top: `${p.y}%`, width: `${p.s * 3}rem`, animationDelay: `${p.d}s` }}
          aria-hidden
        >
          <path d="M12 1.5l2.3 8.2 8.2 2.3-8.2 2.3L12 22.5l-2.3-8.2-8.2-2.3 8.2-2.3z" fill="currentColor" />
        </svg>
      ))}
      <svg className="splash__heart" viewBox="0 0 100 92" aria-hidden>
        <path
          className="splash__fill"
          d="M50 86S8 60 8 31A22 22 0 0150 20a22 22 0 0142 11c0 29-42 55-42 55z"
        />
        <path
          className="splash__line"
          pathLength={1}
          d="M50 86S8 60 8 31A22 22 0 0150 20a22 22 0 0142 11c0 29-42 55-42 55z"
        />
      </svg>
      <div className="splash__name" aria-hidden>
        {"CarlaOS".split("").map((letter, i) => (
          <span key={i} style={{ animationDelay: `${0.9 + i * 0.07}s` }}>
            {letter}
          </span>
        ))}
      </div>
    </div>
  );
}
