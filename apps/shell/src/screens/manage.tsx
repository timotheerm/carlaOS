// Administrator page for games added by hand: any program on the Ally can get
// a card, whatever store it came from or none.
import { useEffect, useLayoutEffect, useState } from "react";
import { backend } from "../cleanup/backend";
import type { AdminStatus } from "../cleanup/types";
import type { Game, Inspected, Kind, Match, NewGame, Saves } from "../data/types";
import { library, message } from "../library/store";
import { focusFirst, keepFocus } from "../nav/focus";
import { size, when } from "../ui/parts";
import { PinPad } from "../ui/pin";
import { ArtList, ArtPicker } from "./artwork";
import { Consoles } from "./consoles";

const KINDS: { id: Kind; label: string }[] = [
  { id: "game", label: "Game" },
  { id: "port", label: "Native port" },
  { id: "homebrew", label: "Homebrew" },
];

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

// Consoles a port is most often from, to pick with one press.
const ORIGINS = ["Nintendo 64", "Super Nintendo", "GameCube", "Game Boy Advance", "PlayStation", "PlayStation 2", "Mega Drive"];

function AddForm({ seen, onDone, notify }: { seen: Inspected; onDone: (added?: { id: string; needsPicture: boolean }) => void; notify: (text: string) => void }) {
  const exe = seen.exe;
  const [game, setGame] = useState<NewGame>({ title: seen.title, exe, args: seen.args, kind: seen.kind, cover: null, description: "", origin: seen.origin });
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
      const id = await library.addManual({ ...game, title: game.title.trim(), origin: game.kind === "port" ? game.origin.trim() : "" });
      notify(game.cover ? `${game.title.trim()} was added.` : `${game.title.trim()} was added. Now choose its picture.`);
      onDone({ id, needsPicture: !game.cover });
    } catch (e) {
      notify(message(e));
      setSaving(false);
    }
  };

  return (
    <div className="form">
      <div className="pagehead">
        <h1>Add a game</h1>
        <p>
          {fileName(exe)}
          {seen.shortcut ? " · the program behind the shortcut you chose" : ""}
          {seen.port ? ` · recognised as ${seen.port}, a native port` : ""}
        </p>
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
      {game.kind === "port" && (
        <div className="form__row">
          <span>First came out on</span>
          <div className="chips">
            <input
              className="field field--short"
              data-focus
              data-fid="add:origin"
              value={game.origin}
              maxLength={40}
              placeholder="A console, or empty"
              onChange={(e) => setGame({ ...game, origin: e.target.value })}
            />
            {ORIGINS.filter((o) => o !== game.origin).slice(0, 5).map((o) => (
              <button key={o} className="chip" data-focus data-fid={`add:origin:${o}`} onClick={() => setGame({ ...game, origin: o })}>
                {o}
              </button>
            ))}
          </div>
        </div>
      )}
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
                <small>{m.from}</small>
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
        <button className="pill" data-focus data-fid="add:cancel" onClick={() => onDone()}>
          Cancel
        </button>
        </div>
      </div>
    </div>
  );
}

// A cloud game: a link to its page on its service.
function CloudForm({ onDone, notify }: { onDone: (added?: string) => void; notify: (text: string) => void }) {
  const [link, setLink] = useState("");
  const [title, setTitle] = useState("");
  const [service, setService] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  // Read the link as it is pasted: which service, and a name when it has one.
  useEffect(() => {
    const typed = link.trim();
    if (!typed) return setService(null);
    let current = true;
    library.inspectCloud(typed).then(
      (seen) => {
        if (!current) return;
        setService(seen.service);
        setTitle((t) => t || seen.title);
      },
      () => current && setService(null),
    );
    return () => {
      current = false;
    };
  }, [link]);

  const save = async () => {
    setSaving(true);
    try {
      const id = await library.addCloud(link.trim(), title.trim());
      notify(`${title.trim()} was added. Now choose its picture.`);
      onDone(id);
    } catch (e) {
      notify(message(e));
      setSaving(false);
    }
  };

  return (
    <div className="form">
      <div className="pagehead">
        <h1>Add a cloud game</h1>
        <p>Open the game's page on its service in a browser, copy the address, and paste it here. From a desktop it is easier: carla-companion ally cloud LINK</p>
      </div>
      <label className="form__row">
        <span>Link</span>
        <input
          className="field"
          data-focus
          data-autofocus
          data-fid="cloud:link"
          value={link}
          maxLength={2048}
          placeholder="https://www.xbox.com/play/games/…"
          onChange={(e) => setLink(e.target.value)}
        />
      </label>
      <div className="form__row">
        <span>Plays on</span>
        <p className="form__found">{service ?? (link.trim() ? "Not a link CarlaOS knows: GeForce NOW, Xbox Cloud Gaming, Boosteroid or Luna" : "Paste a link first")}</p>
      </div>
      <label className="form__row">
        <span>Name</span>
        <input className="field" data-focus data-fid="cloud:title" value={title} maxLength={80} onChange={(e) => setTitle(e.target.value)} />
      </label>
      <div className="form__row">
        <span />
        <div className="form__actions">
          <button className="pill pill--primary" data-focus data-fid="cloud:save" disabled={saving || !service || !title.trim()} onClick={() => void save()}>
            Add to library
          </button>
          <button className="pill" data-focus data-fid="cloud:cancel" onClick={() => onDone()}>
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
  const [adding, setAdding] = useState<Inspected | null>(null); // program being added
  const [art, setArt] = useState<string | "list" | null>(null); // game whose pictures are open
  const [saves, setSaves] = useState<string | "list" | null>(null); // game whose saves are open
  const [consoles, setConsoles] = useState(false);
  const [addingCloud, setAddingCloud] = useState(false);

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
  }, [admin?.unlocked, adding, addingCloud, saves, consoles, art, manual.length]);

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
  if (adding) {
    return (
      <AddForm
        seen={adding}
        onDone={(added) => {
          setAdding(null);
          // Straight on to its picture, which is the next thing it needs.
          if (added?.needsPicture) setArt(added.id);
        }}
        notify={notify}
      />
    );
  }
  if (addingCloud) {
    return (
      <CloudForm
        onDone={(id) => {
          setAddingCloud(false);
          if (id) setArt(id);
        }}
        notify={notify}
      />
    );
  }
  const pictured = games.find((g) => g.id === art);
  if (pictured) return <ArtPicker game={pictured} onDone={() => setArt("list")} notify={notify} />;
  if (art === "list") return <ArtList games={games} onOpen={setArt} onDone={() => setArt(null)} />;
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
        <p>Games from Steam, Epic and GOG appear by themselves. Add anything else here.</p>
      </div>
      <button
        className="setting"
        data-focus
        data-autofocus
        data-fid="manage:add"
        onClick={async () => {
          try {
            const chosen = await library.pick("program");
            // A shortcut is followed to its program, and a port CarlaOS knows gets its real name.
            if (chosen) setAdding(await library.inspect(chosen));
          } catch (e) {
            notify(message(e));
          }
        }}
      >
        <span>Add a game</span>
        <span className="setting__value">Choose its program, or a shortcut to it</span>
      </button>
      <button className="setting" data-focus data-fid="manage:cloud" onClick={() => setAddingCloud(true)}>
        <span>Add a cloud game</span>
        <span className="setting__value">A game on GeForce NOW, Xbox Cloud Gaming, Boosteroid or Luna</span>
      </button>
      <button className="setting" data-focus data-fid="manage:art" onClick={() => setArt("list")}>
        <span>Game artwork</span>
        <span className="setting__value">
          {games.some((g) => !(g.cover ?? g.hero)) ? `${games.filter((g) => !(g.cover ?? g.hero)).length} without a picture · ` : ""}Find pictures and choose between them
        </span>
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
