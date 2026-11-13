// Administrator pages for game pictures: which games have none, and for one
// game, several pictures found online to choose from.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ArtChoice, ArtSearch, Game } from "../data/types";
import { library, message } from "../library/store";
import { keepFocus } from "../nav/focus";
import { Cover } from "../ui/parts";

const hasPicture = (g: Game) => !!(g.cover ?? g.hero);

/** Every game, the ones without a picture first. */
export function ArtList({ games, onOpen, onDone }: { games: Game[]; onOpen: (id: string) => void; onDone: () => void }) {
  const sorted = [...games].sort((a, b) => Number(hasPicture(a)) - Number(hasPicture(b)) || a.title.localeCompare(b.title));
  const without = games.filter((g) => !hasPicture(g)).length;
  return (
    <div className="settings">
      <div className="pagehead">
        <h1>Game artwork</h1>
        <p>
          {without === 0
            ? "Every game has a picture. Choose a game to give it another."
            : `${without} ${without === 1 ? "game has" : "games have"} no picture yet. Choose a game to find some.`}
        </p>
      </div>
      <button className="setting" data-focus data-fid="art:back" onClick={onDone}>
        <span>Done</span>
        <span className="setting__value">Back to Manage games</span>
      </button>
      {sorted.map((g, i) => (
        <button key={g.id} className="setting setting--art" data-focus data-autofocus={i === 0 ? "" : undefined} data-fid={`art:${g.id}`} onClick={() => onOpen(g.id)}>
          <span className="setting__thumb">
            <Cover game={g} band={false} />
          </span>
          <span>{g.title}</span>
          <span className="setting__value">{!hasPicture(g) ? "No picture yet" : g.ownArt ? "Chosen by you" : g.platform}</span>
        </button>
      ))}
    </div>
  );
}

function Choices({ title, list, busy, onUse }: { title: string; list: ArtChoice[]; busy: string | null; onUse: (c: ArtChoice) => void }) {
  if (list.length === 0) return null;
  return (
    <section className="row">
      <h2 className="row__title">{title}</h2>
      <div className={`arts${list[0].candidate.shape === "wide" ? " arts--wide" : ""}`}>
        {list.map((c) => (
          <button
            key={c.candidate.full}
            className={busy === c.candidate.full ? "art art--busy" : "art"}
            data-focus
            data-fid={`art:pick:${c.candidate.full}`}
            disabled={busy !== null}
            aria-label={`${c.candidate.label}, from ${c.candidate.source}`}
            onClick={() => onUse(c)}
          >
            <img src={c.preview} alt="" draggable={false} />
            <span className="art__from">{c.candidate.source}</span>
          </button>
        ))}
      </div>
    </section>
  );
}

/** Find pictures for one game and choose between them. */
export function ArtPicker({ game, onDone, notify }: { game: Game; onDone: () => void; notify: (text: string) => void }) {
  const [term, setTerm] = useState(game.title);
  const [found, setFound] = useState<ArtSearch | "looking" | null>(null);
  const [busy, setBusy] = useState<string | null>(null); // the picture being fetched
  const [key, setKey] = useState<string | null>(null); // the key being typed, when the field is open
  const latest = useRef(0);

  const look = (name: string) => {
    const mine = ++latest.current;
    setFound("looking");
    library.findArt(game.id, name.trim() === game.title ? "" : name.trim()).then(
      (result) => mine === latest.current && setFound(result),
      (e) => {
        if (mine !== latest.current) return;
        setFound(null);
        notify(message(e));
      },
    );
  };

  // Looked for at once under the game's own name.
  useEffect(() => look(game.title), [game.id]);
  useLayoutEffect(() => keepFocus("art:look"), [found, key === null]);

  const use = (choice: ArtChoice) => {
    setBusy(choice.candidate.full);
    library
      .useArt(game.id, choice)
      .then(
        () => notify(choice.candidate.shape === "wide" ? "That is its wide picture now." : "That is its cover now."),
        (e) => notify(message(e)),
      )
      .finally(() => setBusy(null));
  };

  const pick = (wide: boolean) =>
    library.pickArt(game.id, wide).then(
      (chosen) => chosen && notify(wide ? "That is its wide picture now." : "That is its cover now."),
      (e) => notify(message(e)),
    );

  const results = found !== null && found !== "looking" ? found : null;
  const tall = results?.candidates.filter((c) => c.candidate.shape === "tall") ?? [];
  const wide = results?.candidates.filter((c) => c.candidate.shape === "wide") ?? [];

  return (
    <div className="artpage" data-game={game.id}>
      <div className="artpage__head">
        <div className="artpage__now">
          <Cover game={game} />
        </div>
        <div className="artpage__text">
          <h1>{game.title}</h1>
          <p className="lede">
            A cover is the tall picture on the game's card. A wide picture fills its page and the top of Home. Choose either, or both.
          </p>
          <div className="artpage__search">
            <input
              className="field"
              data-focus
              data-fid="art:term"
              value={term}
              maxLength={80}
              placeholder="The game's name"
              onChange={(e) => setTerm(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && term.trim() && look(term)}
            />
            <button className="pill pill--primary" data-focus data-autofocus data-fid="art:look" disabled={found === "looking" || !term.trim()} onClick={() => look(term)}>
              {found === "looking" ? "Looking…" : "Look for pictures"}
            </button>
          </div>
          <div className="detail__actions">
            <button className="pill" data-focus data-fid="art:own" onClick={() => void pick(false)}>
              Use a cover from this Ally
            </button>
            <button className="pill" data-focus data-fid="art:own-wide" onClick={() => void pick(true)}>
              Use a wide picture from this Ally
            </button>
            {game.ownArt && (
              <button
                className="pill"
                data-focus
                data-fid="art:clear"
                onClick={() => library.clearArt(game.id).then(() => notify("Back to the picture it had by itself."), (e) => notify(message(e)))}
              >
                Back to its own picture
              </button>
            )}
            <button className="pill" data-focus data-fid="art:done" onClick={onDone}>
              Done
            </button>
          </div>
        </div>
      </div>

      {found === "looking" && <p className="lede">Asking Steam, Wikipedia and the box art collections…</p>}
      {results && results.candidates.length === 0 && (
        <p className="lede">Nothing was found under that name. Try a shorter one, or the name the game is best known by.</p>
      )}
      {results?.notes.map((note) => (
        <p key={note} className="notice notice--warn">
          {note}
        </p>
      ))}
      <Choices title="Covers" list={tall} busy={busy} onUse={use} />
      <Choices title="Wide pictures" list={wide} busy={busy} onUse={use} />

      {results && (
        <section className="row">
          <h2 className="row__title">More pictures</h2>
          {key === null ? (
            <button className="setting" data-focus data-fid="art:key" onClick={() => setKey("")}>
              <span>{results.keySet ? "SteamGridDB is being asked too" : "Add a SteamGridDB key"}</span>
              <span className="setting__value">
                {results.keySet ? "Change or forget the key" : "Thousands of covers made by players. The key is free: steamgriddb.com, Preferences, API"}
              </span>
            </button>
          ) : (
            <div className="artpage__search">
              <input
                className="field"
                data-focus
                data-fid="art:key-field"
                value={key}
                maxLength={64}
                placeholder={results.keySet ? "A new key, or leave empty to forget it" : "Paste the key here"}
                onChange={(e) => setKey(e.target.value.trim())}
              />
              <button
                className="pill pill--primary"
                data-focus
                data-fid="art:key-save"
                onClick={() =>
                  !key && !results.keySet ? setKey(null) :
                  library.setArtKey(key).then(
                    (kept) => {
                      setKey(null);
                      notify(kept ? "The key is kept. Looking again." : "The key was forgotten.");
                      look(term);
                    },
                    (e) => notify(message(e)),
                  )
                }
              >
                {key ? "Keep this key" : results.keySet ? "Forget the key" : "Cancel"}
              </button>
            </div>
          )}
        </section>
      )}
    </div>
  );
}
