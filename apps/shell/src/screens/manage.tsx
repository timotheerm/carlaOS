// Administrator page for games added by hand: any program on the Ally can get
// a card, whatever store it came from or none.
import { useEffect, useLayoutEffect, useState } from "react";
import { backend } from "../cleanup/backend";
import type { AdminStatus } from "../cleanup/types";
import type { Game, Kind, Match, NewGame, Saves } from "../data/types";
import { library, message } from "../library/store";
import { focusFirst, keepFocus } from "../nav/focus";
import { size, when } from "../ui/parts";
import { PinPad } from "../ui/pin";
import { Consoles } from "./consoles";

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
  const [game, setGame] = useState<NewGame>({ title: suggestTitle(exe), exe, args: "", kind: "game", cover: null, description: "" });
  const [saving, setSaving] = useState(false);
  // Looking the game up online: nothing yet, looking, the matches, or what was found.
  const [found, setFound] = useState<Match[] | "looking" | null>(null);
  const [picture, setPicture] = useState<string | null>(null);

  // The button pressed is replaced by the matches, and the matches by the button.
  useLayoutEffect(() => keepFocus(Array.isArray(found) && found[0] ? `add:match:${found[0].id}` : "add:find"), [found]);

  const find = async () => {
    setFound("looking");
    try {
      const matches = await library.findOnline(game.title.trim());
      setFound(matches.slice(0, 5));
      if (matches.length === 0) notify("Nothing by that name was found. Try a shorter name.");
    } catch (e) {
      setFound(null);
      notify(message(e));
    }
  };

  const use = async (match: Match) => {
    setFound("looking");
    try {
      const got = await library.fetchOnline(match.id);
      setGame((g) => ({ ...g, description: got.description, cover: got.cover ?? g.cover }));
      setPicture(got.preview);
      notify(got.cover ? "Description and cover added." : "Description added.");
    } catch (e) {
      notify(message(e));
    }
    setFound(null);
  };

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
        <span>Details</span>
        <div className="chips">
          {Array.isArray(found) && found.length > 0 ? (
            found.map((m) => (
              <button key={m.id} className="chip" data-focus data-fid={`add:match:${m.id}`} onClick={() => void use(m)}>
                {m.name}
              </button>
            ))
          ) : (
            <button className="pill" data-focus data-fid="add:find" disabled={found === "looking" || !game.title.trim()} onClick={() => void find()}>
              {found === "looking" ? "Looking…" : game.description ? "Look again online" : "Find details online"}
            </button>
          )}
        </div>
      </div>
      {game.description && (
        <div className="form__row">
          <span />
          <p className="form__found">
            {picture && <img src={picture} alt="" />}
            {game.description}
          </p>
        </div>
      )}
      <div className="form__row">
        <span>Artwork</span>
        <button
          className="pill"
          data-focus
          data-fid="add:cover"
          onClick={async () => {
            const picture = await library.pick("image").catch(() => null);
            if (picture) {
              setGame((g) => ({ ...g, cover: picture }));
              setPicture(null);
            }
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

// Where one game keeps its saves, and putting a safe copy back.
function SavesAdmin({ game, onDone, notify }: { game: Game; onDone: () => void; notify: (text: string) => void }) {
  const [saves, setSaves] = useState<Saves | null | undefined>(undefined);
  const [sure, setSure] = useState<string | null>(null); // backup about to be put back

  useEffect(() => {
    library.details(game.id).then((d) => setSaves(d.saves), () => setSaves(null));
  }, [game.id]);

  useLayoutEffect(() => keepFocus("saves:pick"), [saves]);

  const attempt = (work: Promise<Saves | null>, done: string) =>
    work.then(
      (next) => {
        if (next) {
          setSaves(next);
          notify(done);
        }
      },
      (e) => notify(message(e)),
    );

  return (
    <div className="settings">
      <div className="pagehead">
        <h1>{game.title}</h1>
        <p>
          {saves === undefined
            ? "Looking…"
            : saves
              ? `${saves.folder} · ${saves.files} ${saves.files === 1 ? "file" : "files"} · ${size(saves.bytes)}`
              : "CarlaOS does not know where this game keeps its saves."}
        </p>
      </div>
      <button
        className="setting"
        data-focus
        data-autofocus
        data-fid="saves:pick"
        onClick={() => void attempt(library.pickSaveFolder(game.id), "The save folder was set.")}
      >
        <span>{saves ? "Choose another save folder" : "Choose the save folder"}</span>
        <span className="setting__value">The folder with only this game's saves</span>
      </button>
      {saves && (
        <button className="setting" data-focus data-fid="saves:now" onClick={() => void attempt(library.backupSaves(game.id), "A safe copy was made.")}>
          <span>Make a safe copy now</span>
          <span className="setting__value">{saves.backups.length} kept, 5 at most</span>
        </button>
      )}
      {saves?.backups.map((b) => (
        <button
          key={b.id}
          className="setting"
          data-focus
          data-fid={`saves:restore:${b.id}`}
          onBlur={() => setSure(null)}
          onClick={() => {
            if (sure !== b.id) return setSure(b.id);
            setSure(null);
            void attempt(library.restoreSaves(game.id, b.id), "The saves were put back. The ones from before are kept as a safe copy.");
          }}
        >
          <span>Safe copy from {when(b.at)}</span>
          <span className="setting__value">{sure === b.id ? "Press again to put it back" : `${b.files} files · ${size(b.bytes)} · Put back`}</span>
        </button>
      ))}
      {saves && (
        <button
          className="setting"
          data-focus
          data-fid="saves:forget"
          onClick={() =>
            library.forgetSaveFolder(game.id).then(
              () => library.details(game.id).then((d) => {
                setSaves(d.saves);
                notify(d.saves ? "Back to the folder CarlaOS found by itself." : "The save folder was forgotten. Safe copies are kept.");
              }),
              (e) => notify(message(e)),
            )
          }
        >
          <span>Forget the chosen folder</span>
          <span className="setting__value">Safe copies are kept</span>
        </button>
      )}
      <button className="setting" data-focus data-fid="saves:done" onClick={onDone}>
        <span>Done</span>
        <span className="setting__value">Back to Manage games</span>
      </button>
    </div>
  );
}

export function Manage({ games, notify }: { games: Game[]; notify: (text: string) => void }) {
  const [admin, setAdmin] = useState<AdminStatus | null>(null);
  const [adding, setAdding] = useState<string | null>(null); // program being added
  const [saves, setSaves] = useState<string | "list" | null>(null); // game whose saves are open
  const [consoles, setConsoles] = useState(false);

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
  }, [admin?.unlocked, adding, saves, consoles, manual.length]);

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
  if (consoles) return <Consoles onDone={() => setConsoles(false)} notify={notify} />;
  const onAlly = games.filter((g) => g.places.includes("local")).sort((a, b) => a.title.localeCompare(b.title));
  const open = onAlly.find((g) => g.id === saves);
  if (open) return <SavesAdmin game={open} onDone={() => setSaves("list")} notify={notify} />;
  if (saves === "list") {
    return (
      <div className="settings">
        <div className="pagehead">
          <h1>Game saves</h1>
          <p>Choose a game to say where its saves are, or to put a safe copy back.</p>
        </div>
        <button className="setting" data-focus data-autofocus data-fid="saves:back" onClick={() => setSaves(null)}>
          <span>Done</span>
          <span className="setting__value">Back to Manage games</span>
        </button>
        {onAlly.map((g) => (
          <button key={g.id} className="setting" data-focus data-fid={`saves:${g.id}`} onClick={() => setSaves(g.id)}>
            <span>{g.title}</span>
            <span className="setting__value">{g.platform}</span>
          </button>
        ))}
      </div>
    );
  }

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
      <button className="setting" data-focus data-fid="manage:consoles" onClick={() => setConsoles(true)}>
        <span>Consoles and emulators</span>
        <span className="setting__value">Your own game files</span>
      </button>
      <button className="setting" data-focus data-fid="manage:saves" onClick={() => setSaves("list")}>
        <span>Game saves</span>
        <span className="setting__value">Save folders and safe copies</span>
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
