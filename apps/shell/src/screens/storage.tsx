// Administrator page: what fills the Ally's disk, and room for new games.
// Steam itself asks before it deletes anything.
import { useCallback, useEffect, useLayoutEffect, useState } from "react";
import { backend } from "../cleanup/backend";
import type { AdminStatus } from "../cleanup/types";
import type { Storage as StorageView } from "../data/types";
import { library, message } from "../library/store";
import { focusFirst } from "../nav/focus";
import { size, when } from "../ui/parts";
import { PinPad } from "../ui/pin";

function DiskBar({ free, total, folder }: { free: number; total: number; folder: string }) {
  const used = total ? Math.round(((total - free) / total) * 100) : 0;
  return (
    <div className="disk">
      <div className="disk__text">
        <b>{size(free)} free</b>
        <span>
          of {size(total)} · {folder}
        </span>
      </div>
      <span className={used >= 90 ? "progress progress--full" : "progress"} role="img" aria-label={`${used} % used`}>
        <i style={{ width: `${used}%` }} />
      </span>
    </div>
  );
}

export function Storage({ notify }: { notify: (text: string) => void }) {
  const [admin, setAdmin] = useState<AdminStatus | null>(null);
  const [view, setView] = useState<StorageView | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  // The game whose uninstall is asked once, waiting for a second press.
  const [asking, setAsking] = useState<string | null>(null);

  const load = useCallback(() => {
    library.storage().then(
      (v) => {
        setView(v);
        setProblem(null);
      },
      (e) => setProblem(message(e)),
    );
  }, []);

  useEffect(() => {
    void backend.status().then(setAdmin, () => setAdmin({ pin_set: false, unlocked: false }));
    load();
    // A game that Steam finished removing changes the list.
    const again = setInterval(load, 8_000);
    return () => {
      clearInterval(again);
      void backend.lock().catch(() => undefined);
    };
  }, [load]);

  const unlocked = admin?.unlocked === true;
  useLayoutEffect(() => {
    const main = document.querySelector("main");
    if (main) focusFirst(main);
  }, [unlocked]);

  if (!admin) return null;
  if (!admin.unlocked) {
    return (
      <PinPad
        pinSet={admin.pin_set}
        lede="Storage is locked."
        unlock={async (pin) => {
          const r = await backend.unlock(pin);
          if (r.result === "unlocked") setAdmin({ pin_set: true, unlocked: true });
          return r;
        }}
        setPin={async (pin) => {
          await backend.setPin(pin);
          setAdmin({ pin_set: true, unlocked: true });
        }}
      />
    );
  }

  return (
    <div className="settings">
      <div className="pagehead">
        <h1>Storage</h1>
        <p>Games installed through Steam on this Ally, largest first. Uninstalling one makes room for another from the library.</p>
      </div>
      {problem && <div className="notice notice--warn">{problem}</div>}
      {view?.disks.map((d) => <DiskBar key={d.folder} {...d} />)}
      {view && view.games.length === 0 && <div className="notice">No Steam games are installed on this Ally.</div>}
      {view?.games.map((g, i) => (
        <button
          key={g.game_id}
          className={asking === g.game_id ? "setting setting--danger" : "setting"}
          data-focus
          data-autofocus={i === 0 ? "" : undefined}
          data-fid={`store:${g.game_id}`}
          onBlur={() => setAsking((a) => (a === g.game_id ? null : a))}
          onClick={() => {
            if (asking !== g.game_id) {
              setAsking(g.game_id);
              return;
            }
            setAsking(null);
            library.uninstall(g.game_id).then(
              () => notify(`Steam is asking whether to uninstall ${g.title}. Answer in Steam's window.`),
              (e) => notify(message(e)),
            );
          }}
        >
          <span>{g.title}</span>
          <span className="setting__value">
            {asking === g.game_id
              ? "Press again to uninstall"
              : `${size(g.bytes)} · ${g.last_played ? `played ${when(g.last_played)}` : "not played from CarlaOS"}`}
          </span>
        </button>
      ))}
      <div className="notice">
        <b>Nothing is lost</b>
        <span>An uninstalled game stays in the library under Not installed and can be installed again at any time.</span>
        <span>Steam keeps cloud saves for most games. For the others, CarlaOS keeps the safe copies it made.</span>
      </div>
    </div>
  );
}
