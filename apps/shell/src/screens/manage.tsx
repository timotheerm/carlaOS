// Administrator page for games added by hand: any program on the Ally can get
// a card, whatever store it came from or none.
import { useEffect, useLayoutEffect, useState } from "react";
import { backend } from "../cleanup/backend";
import type { AdminStatus } from "../cleanup/types";
import type { Game, Kind, NewGame } from "../data/types";
import { library, message } from "../library/store";
import { focusFirst } from "../nav/focus";
import { PinPad } from "../ui/pin";

const KINDS: { id: Kind; label: string }[] = [
  { id: "game", label: "Game" },
  { id: "port", label: "Native port" },
  { id: "homebrew", label: "Homebrew" },
];

// "C:\Games\Paper Lanterns\lanterns.exe" suggests "Paper Lanterns".
function suggestTitle(exe: string): string {
  const parts = exe.split(/[\\/]/).filter(Boolean);
  const file = (parts.pop() ?? "").replace(/\.[^.]+$/, "");
  const folder = parts.pop() ?? "";
  const generic = /^(game|launcher|start|play|bin|win64|x64|binaries)$/i;
  // The folder usually carries the nicer spelling of the same name.
  const squash = (t: string) => t.toLowerCase().replace(/[^a-z0-9]/g, "");
  const pick = generic.test(file) || file.length < 3 || squash(file) === squash(folder) ? folder || file : file;
  return pick.replace(/[_-]+/g, " ").trim();
}

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

function AddForm({ exe, onDone, notify }: { exe: string; onDone: () => void; notify: (text: string) => void }) {
  const [game, setGame] = useState<NewGame>({ title: suggestTitle(exe), exe, args: "", kind: "game", cover: null });
  const [saving, setSaving] = useState(false);

  const save = async () => {
    setSaving(true);
    try {
      await library.addManual({ ...game, title: game.title.trim() });
      notify(`${game.title.trim()} was added.`);
      onDone();
    } catch (e) {
      notify(message(e));
      setSaving(false);
    }
  };

  return (
    <div className="form">
      <div className="pagehead">
        <h1>Add a game</h1>
        <p>{fileName(exe)}</p>
      </div>
      <label className="form__row">
        <span>Name</span>
        <input
          className="field"
          data-focus
          data-autofocus
          data-fid="add:title"
          value={game.title}
          maxLength={80}
          onChange={(e) => setGame({ ...game, title: e.target.value })}
        />
      </label>
      <div className="form__row">
        <span>Shown under</span>
        <div className="chips">
          {KINDS.map((k) => (
            <button
              key={k.id}
              className={k.id === game.kind ? "chip chip--on" : "chip"}
              data-focus
              data-fid={`add:kind:${k.id}`}
              onClick={() => setGame({ ...game, kind: k.id })}
            >
              {k.label}
            </button>
          ))}
        </div>
      </div>
      <label className="form__row">
        <span>Launch options</span>
        <input
          className="field"
          data-focus
          data-fid="add:args"
          value={game.args}
          placeholder="Usually empty"
          onChange={(e) => setGame({ ...game, args: e.target.value })}
        />
      </label>
      <div className="form__row">
        <span>Artwork</span>
        <button
          className="pill"
          data-focus
          data-fid="add:cover"
          onClick={async () => {
            const picture = await library.pick("image").catch(() => null);
            if (picture) setGame((g) => ({ ...g, cover: picture }));
          }}
        >
          {game.cover ? fileName(game.cover) : "Choose a picture"}
        </button>
      </div>
      <div className="form__row">
        <span />
        <div className="form__actions">
        <button className="pill pill--primary" data-focus data-fid="add:save" disabled={saving || !game.title.trim()} onClick={() => void save()}>
          Add to library
        </button>
        <button className="pill" data-focus data-fid="add:cancel" onClick={onDone}>
          Cancel
        </button>
        </div>
      </div>
    </div>
  );
}

export function Manage({ games, notify }: { games: Game[]; notify: (text: string) => void }) {
  const [admin, setAdmin] = useState<AdminStatus | null>(null);
  const [adding, setAdding] = useState<string | null>(null); // program being added

  // Locked on the way in and again on the way out.
  useEffect(() => {
    void backend.status().then(setAdmin, () => setAdmin({ pin_set: false, unlocked: false }));
    return () => void backend.lock().catch(() => undefined);
  }, []);

  const manual = games.filter((g) => g.manual);
  useLayoutEffect(() => {
    const active = document.activeElement;
    if (active instanceof HTMLElement && active.matches("[data-focus]") && active.isConnected) return;
    const main = document.querySelector("main");
    if (main) focusFirst(main);
  }, [admin?.unlocked, adding, manual.length]);

  if (!admin) return null;
  if (!admin.unlocked) {
    return (
      <PinPad
        pinSet={admin.pin_set}
        lede="Adding and removing games is locked."
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
  if (adding) return <AddForm exe={adding} onDone={() => setAdding(null)} notify={notify} />;

  return (
    <div className="settings">
      <div className="pagehead">
        <h1>Manage games</h1>
        <p>Steam games appear by themselves. Add anything else here.</p>
      </div>
      <button
        className="setting"
        data-focus
        data-autofocus
        data-fid="manage:add"
        onClick={async () => {
          try {
            const exe = await library.pick("program");
            if (exe) setAdding(exe);
          } catch (e) {
            notify(message(e));
          }
        }}
      >
        <span>Add a game</span>
        <span className="setting__value">Choose its program</span>
      </button>
      {manual.map((g) => (
        <button
          key={g.id}
          className="setting"
          data-focus
          data-fid={`manage:${g.id}`}
          onClick={() => library.removeManual(g.id).then(() => notify(`${g.title} was removed.`), (e) => notify(message(e)))}
        >
          <span>{g.title}</span>
          <span className="setting__value">Remove from library</span>
        </button>
      ))}
    </div>
  );
}
