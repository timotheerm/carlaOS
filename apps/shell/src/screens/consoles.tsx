// Administrator pages for consoles: an emulator that is already installed,
// paired with a folder of the administrator's own game files. CarlaOS brings
// no emulator, no system files and no games.
import { useEffect, useLayoutEffect, useState } from "react";
import type { Console } from "../data/types";
import { library, message } from "../library/store";
import { keepFocus } from "../nav/focus";

const fileName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;
const blank: Console = { id: "", name: "", emulator: "", options: '"{game}"', folder: "", endings: [] };

function Form({ start, onDone, notify }: { start: Console; onDone: () => void; notify: (text: string) => void }) {
  const [console_, setConsole] = useState<Console>(start);
  const [endings, setEndings] = useState(start.endings.join(", "));
  const [saving, setSaving] = useState(false);
  const ready = console_.name.trim() && console_.emulator && console_.folder && endings.trim();

  const pickEmulator = async () => {
    const emulator = await library.pick("emulator").catch(() => null);
    if (!emulator) return;
    // A known emulator fills in what is still empty; nothing typed is replaced.
    const hint = await library.suggestConsole(emulator).catch(() => null);
    setConsole((c) => ({
      ...c,
      emulator,
      name: c.name.trim() || hint?.name || "",
      options: c.options === blank.options && hint ? hint.options : c.options,
    }));
    if (hint && !endings.trim()) setEndings(hint.endings.join(", "));
  };

  const save = async () => {
    setSaving(true);
    try {
      const games = await library.saveConsole({ ...console_, endings: endings.split(/[\s,;]+/).filter(Boolean) });
      notify(`${console_.name.trim()}: ${games} ${games === 1 ? "game" : "games"} found.`);
      onDone();
    } catch (e) {
      notify(message(e));
      setSaving(false);
    }
  };

  return (
    <div className="form">
      <div className="pagehead">
        <h1>{start.id ? start.name : "Add a console"}</h1>
        <p>An emulator you have installed, and the folder with your own game files for it.</p>
      </div>
      <div className="form__row">
        <span>Emulator</span>
        <button className="pill" data-focus data-autofocus data-fid="con:emulator" onClick={() => void pickEmulator()}>
          {console_.emulator ? fileName(console_.emulator) : "Choose its program"}
        </button>
      </div>
      <label className="form__row">
        <span>Name on the badge</span>
        <input
          className="field"
          data-focus
          data-fid="con:name"
          value={console_.name}
          maxLength={40}
          placeholder="GameCube, PlayStation 2…"
          onChange={(e) => setConsole({ ...console_, name: e.target.value })}
        />
      </label>
      <div className="form__row">
        <span>Games folder</span>
        <button
          className="pill"
          data-focus
          data-fid="con:folder"
          onClick={async () => {
            const folder = await library.pick("folder").catch(() => null);
            if (folder) setConsole((c) => ({ ...c, folder }));
          }}
        >
          {console_.folder ? fileName(console_.folder) : "Choose the folder"}
        </button>
      </div>
      <label className="form__row">
        <span>Game file endings</span>
        <input
          className="field"
          data-focus
          data-fid="con:endings"
          value={endings}
          placeholder="iso, chd"
          onChange={(e) => setEndings(e.target.value)}
        />
      </label>
      <label className="form__row">
        <span>Emulator options</span>
        <input
          className="field"
          data-focus
          data-fid="con:options"
          value={console_.options}
          onChange={(e) => setConsole({ ...console_, options: e.target.value })}
        />
      </label>
      <div className="form__row">
        <span />
        <p className="form__found">{"{game}"} is where the game file goes. For an emulator CarlaOS recognises, the options are filled in for you.</p>
      </div>
      <div className="form__row">
        <span />
        <div className="form__actions">
          <button className="pill pill--primary" data-focus data-fid="con:save" disabled={saving || !ready} onClick={() => void save()}>
            {saving ? "Looking for games…" : start.id ? "Save" : "Add console"}
          </button>
          <button className="pill" data-focus data-fid="con:cancel" onClick={onDone}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

export function Consoles({ onDone, notify }: { onDone: () => void; notify: (text: string) => void }) {
  const [consoles, setConsoles] = useState<Console[] | null>(null);
  const [editing, setEditing] = useState<Console | null>(null);
  const [sure, setSure] = useState<string | null>(null); // console about to be removed

  const load = () => library.consoles().then(setConsoles, (e) => notify(message(e)));
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useLayoutEffect(() => keepFocus("con:add"), [consoles, editing]);

  if (editing) {
    return (
      <Form
        start={editing}
        notify={notify}
        onDone={() => {
          setEditing(null);
          void load();
        }}
      />
    );
  }

  return (
    <div className="settings">
      <div className="pagehead">
        <h1>Consoles and emulators</h1>
        <p>CarlaOS starts emulators you installed with game files you own. It brings no emulator, no system files and no games.</p>
      </div>
      <button className="setting" data-focus data-autofocus data-fid="con:add" onClick={() => setEditing(blank)}>
        <span>Add a console</span>
        <span className="setting__value">Emulator and games folder</span>
      </button>
      {consoles?.map((c) => (
        <div className="setting-pair" key={c.id}>
          <button className="setting" data-focus data-fid={`con:${c.id}`} onClick={() => setEditing(c)}>
            <span>{c.name}</span>
            <span className="setting__value">
              {fileName(c.emulator)} · {fileName(c.folder)} · change
            </span>
          </button>
          <button
            className="setting setting--side"
            data-focus
            data-fid={`con:remove:${c.id}`}
            onBlur={() => setSure(null)}
            onClick={() => {
              if (sure !== c.id) return setSure(c.id);
              setSure(null);
              library.removeConsole(c.id).then(
                () => {
                  notify(`${c.name} was removed. No file was deleted.`);
                  void load();
                },
                (e) => notify(message(e)),
              );
            }}
          >
            {sure === c.id ? "Press again" : "Remove"}
          </button>
        </div>
      ))}
      <button className="setting" data-focus data-fid="con:done" onClick={onDone}>
        <span>Done</span>
        <span className="setting__value">Back to Manage games</span>
      </button>
    </div>
  );
}
