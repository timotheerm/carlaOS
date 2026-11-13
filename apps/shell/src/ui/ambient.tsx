// The whole screen takes on the colours of the game in focus: its picture,
// very soft, behind everything. Two layers take turns so one fades into the
// next. Nothing here can be focused or pressed.
import { useEffect, useRef, useState } from "react";
import type { Game } from "../data/types";

type Look = { key: string; picture?: string; hue: number };

const lookOf = (game: Game): Look => ({ key: game.id, picture: game.hero ?? game.cover, hue: game.hue });

function style(look: Look | null): React.CSSProperties | undefined {
  if (!look) return undefined;
  return look.picture ? { backgroundImage: `url("${look.picture}")` } : ({ "--hue": look.hue } as React.CSSProperties);
}

export function Ambient({ games, fixed }: { games: Game[]; fixed?: Game | null }) {
  // Two layers; `front` says which one shows.
  const [{ layers, front }, setState] = useState<{ layers: [Look | null, Look | null]; front: number }>({ layers: [null, null], front: 0 });
  const shown = useRef<string | null>(null);
  const latest = useRef(games);
  latest.current = games;

  const show = (look: Look | null) => {
    const key = look ? `${look.key}|${look.picture ?? ""}` : null;
    if (key === shown.current) return;
    shown.current = key;
    setState((s) => (s.front === 0 ? { layers: [s.layers[0], look], front: 1 } : { layers: [look, s.layers[1]], front: 0 }));
  };

  // A page that is about one game (it is starting, or running) shows that one.
  useEffect(() => {
    if (fixed !== undefined) show(fixed ? lookOf(fixed) : null);
  }, [fixed]);

  useEffect(() => {
    if (fixed !== undefined) return;
    let timer = 0;
    const onFocus = (e: FocusEvent) => {
      const id = (e.target as HTMLElement | null)?.closest?.<HTMLElement>("[data-game]")?.dataset.game;
      // Moving along a row should not flicker through every game on the way.
      clearTimeout(timer);
      timer = window.setTimeout(() => {
        const game = id ? latest.current.find((g) => g.id === id) : undefined;
        show(game ? lookOf(game) : null);
      }, 140);
    };
    document.addEventListener("focusin", onFocus);
    return () => {
      clearTimeout(timer);
      document.removeEventListener("focusin", onFocus);
    };
  }, [fixed]);

  return (
    <div className="ambient" aria-hidden>
      {layers.map((look, i) => (
        <div
          key={i}
          className={`ambient__layer${look && !look.picture ? " ambient__layer--hue" : ""}${look && front === i ? " ambient__layer--on" : ""}`}
          style={style(look)}
        />
      ))}
      <div className="ambient__veil" />
    </div>
  );
}
