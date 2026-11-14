// The page of one game, shown before it starts, the way a console does: how
// long it has been played, what has been achieved, and how the saves are.
import { useEffect, useState } from "react";
import type { Achievement, Details, Game, SaveSync, Saves } from "../data/types";
import { library, message } from "../library/store";
import { Clock, Heart, Lock, SaveIcon, Sparkle, Trophy } from "../ui/icons";
import { Badges, Cover, Empty, Row, amount, day, placeLabel, size, updateLine, when } from "../ui/parts";
import type { Ctx } from "./screens";

// More than this many tiles in one row stops being a pleasure to scroll.
const MOST = 36;

const earned = (list: Achievement[]) => list.filter((a) => a.unlocked !== null).length;

// Earned first, newest first; then what is still to do; secrets last.
function inOrder(list: Achievement[]): Achievement[] {
  const rank = (a: Achievement) => (a.unlocked !== null ? 0 : a.hidden ? 2 : 1);
  return [...list].sort((a, b) => rank(a) - rank(b) || (b.unlocked ?? 0) - (a.unlocked ?? 0));
}

function Trophies({ list, prefix, milestone = false }: { list: Achievement[]; prefix: string; milestone?: boolean }) {
  const shown = inOrder(list).slice(0, MOST);
  return (
    <>
      {shown.map((a) => {
        const done = a.unlocked !== null;
        const secret = a.hidden && !done;
        return (
          <button
            key={a.id}
            className={done ? "trophy trophy--done" : "trophy"}
            data-focus
            data-fid={`${prefix}:${a.id}`}
            aria-label={`${secret ? "Hidden achievement" : a.name}, ${done ? "earned" : "not earned yet"}`}
          >
            <span className="trophy__icon">{done ? milestone ? <Sparkle /> : <Trophy /> : <Lock />}</span>
            <span className="trophy__name">{secret ? "Hidden" : a.name}</span>
            <span className="trophy__text">{secret ? "Keep playing to find out." : a.description}</span>
            {done && <span className="trophy__when">{a.unlocked ? when(a.unlocked) : "Earned"}</span>}
          </button>
        );
      })}
      {list.length > shown.length && <span className="trophy trophy--more">and {list.length - shown.length} more</span>}
    </>
  );
}

function Meter({ done, of }: { done: number; of: number }) {
  return (
    <span className="meter" role="img" aria-label={`${done} of ${of}`}>
      <i style={{ width: `${of ? Math.round((done / of) * 100) : 0}%` }} />
    </span>
  );
}

function Stat({ icon, label, value, children }: { icon: React.ReactNode; label: string; value: string; children?: React.ReactNode }) {
  return (
    <div className="stat">
      <span className="stat__icon">{icon}</span>
      <span className="stat__value">{value}</span>
      <span className="stat__label">{label}</span>
      {children}
    </div>
  );
}

// One sentence on how the saves stand with a desktop.
function inStep(s: SaveSync): string {
  if (s.outcome === "problem") return `Not in step with ${s.host}: ${s.problem ?? "something went wrong."}`;
  if (s.both_changed) {
    return s.outcome === "fetched"
      ? `Both had new saves. The newer ones, from ${s.host}, are here now; the ones from this Ally are in a safe copy.`
      : `Both had new saves. The newer ones, from this Ally, went to ${s.host}, which kept its own in a safe copy.`;
  }
  if (s.outcome === "fetched") return `Continues from ${s.host} · saves came over ${when(s.at)}`;
  if (s.outcome === "sent") return `Ready to continue on ${s.host} · saves went over ${when(s.at)}`;
  return `In step with ${s.host} · checked ${when(s.at)}`;
}

function SavesPanel({ game, saves, sync, onSaves, notify }: { game: Game; saves: Saves | null; sync: SaveSync[]; onSaves: (s: Saves) => void; notify: Ctx["notify"] }) {
  const [busy, setBusy] = useState(false);
  if (!game.places.includes("local")) {
    return <p className="lede">Saves for this game stay on the desktop it streams from.</p>;
  }
  if (!saves) {
    return <p className="lede">CarlaOS does not know yet where this game keeps its saves. It can be set under Settings, then Manage games.</p>;
  }
  const latest = saves.backups[0];
  return (
    <div className="savebox">
      <div className="savebox__text">
        <b>{saves.modified === null ? "No save yet" : `Last saved ${when(saves.modified)}`}</b>
        <span>
          {saves.files} {saves.files === 1 ? "file" : "files"} · {size(saves.bytes)}
        </span>
        <span>
          {latest
            ? `Safe copy from ${when(latest.at)} · ${saves.backups.length} kept`
            : "No safe copy yet. CarlaOS makes one each time you finish playing."}
        </span>
        {sync.map((s) => (
          <span key={s.host} className={s.outcome === "problem" ? "savebox__sync savebox__sync--problem" : "savebox__sync"}>
            {inStep(s)}
          </span>
        ))}
      </div>
      <button
        className="pill"
        data-focus
        data-fid="saves:backup"
        disabled={busy || saves.files === 0}
        onClick={() => {
          setBusy(true);
          library.backupSaves(game.id).then(
            (next) => {
              onSaves(next);
              notify("A safe copy of the saves was made.");
            },
            (e) => notify(message(e)),
          ).finally(() => setBusy(false));
        }}
      >
        {busy ? "Copying…" : "Make a safe copy now"}
      </button>
    </div>
  );
}

export function GameDetail({ gameId, onToggleFavourite, ...ctx }: Ctx & { gameId: string; onToggleFavourite: (id: string) => void }) {
  const game = ctx.games.find((g) => g.id === gameId);
  const [details, setDetails] = useState<Details | null>(null);
  // Playtime changes when a game ends, and so do achievements and saves.
  const played = game?.playtimeMinutes;
  useEffect(() => {
    let current = true;
    library.details(gameId).then((d) => current && setDetails(d), () => undefined);
    // Meanwhile the saves are brought in step with the desktops, which can
    // take a moment; the page shows the result when there is one.
    library
      .syncSaves(gameId)
      .then((sync) => (sync.length ? library.details(gameId) : null))
      .then((d) => current && d && setDetails(d), () => undefined);
    // Whether a store has an update to finish first is asked afresh too.
    library.updates(gameId).catch(() => undefined);
    return () => {
      current = false;
    };
  }, [gameId, played]);

  if (!game) return <Empty>This game is no longer in the library.</Empty>;
  const fav = ctx.favourites.has(game.id);
  const own = details?.achievements ?? [];
  const milestones = details?.milestones ?? [];
  // The headline number is the game's own achievements when it has them.
  const counted = own.length ? own : milestones;
  const local = game.places.includes("local");
  const from = details?.achievementsFrom;

  return (
    <div className="pregame" data-game={game.id}>
      <article className="detail">
        <Cover game={game} wide />
        <div className="detail__body">
          <Badges game={game} hosts={ctx.hosts} />
          <h1 className="detail__title">{game.title}</h1>
          {game.description && <p className="detail__desc">{game.description}</p>}
          {(game.updates ?? []).map((u) => (
            <p key={u.place} className="detail__update">
              {updateLine(u, ctx.hosts)}
            </p>
          ))}
          <div className="stats">
            <Stat icon={<Clock />} label={game.playtimeMinutes ? "played" : "Not played yet"} value={game.playtimeMinutes ? amount(game.playtimeMinutes) : "New"} />
            <Stat icon={<Sparkle />} label="Last played" value={day(game.lastPlayed ? Math.floor(new Date(game.lastPlayed).getTime() / 1000) : null)} />
            <Stat icon={<Trophy />} label={own.length ? "achievements" : "milestones"} value={details ? `${earned(counted)} / ${counted.length}` : "…"}>
              {details && <Meter done={earned(counted)} of={counted.length} />}
            </Stat>
            {local && (
              <Stat
                icon={<SaveIcon />}
                label={details?.saves ? "last saved" : "saves"}
                value={details?.saves ? day(details.saves.modified) : details ? "Not set" : "…"}
              />
            )}
          </div>
          <div className="detail__actions">
            {game.places.map((place, i) => {
              const host = ctx.hosts.find((h) => h.id === place);
              const away = host && host.state !== "ready" && host.state !== "busy" && host.state !== "unknown";
              const label = place === "local" ? "Play" : `Stream from ${placeLabel(place, ctx.hosts)}`;
              return (
                <button
                  key={place}
                  className={i === 0 ? "pill pill--primary" : "pill"}
                  data-focus
                  data-autofocus={i === 0 ? "" : undefined}
                  data-fid={`play:${place}`}
                  onClick={() => ctx.play(game.id, place)}
                >
                  {label}
                  {away ? (host.state === "not_ready" ? " (needs attention)" : host.canWake ? " (wakes it first)" : " (not answering)") : ""}
                </button>
              );
            })}
            <button className={fav ? "pill pill--fav" : "pill"} data-focus data-fid="fav" onClick={() => onToggleFavourite(game.id)}>
              <Heart />
              {fav ? "Favourite" : "Add to favourites"}
            </button>
          </div>
        </div>
      </article>

      {own.length > 0 && (
        <Row title={`Achievements · ${earned(own)} of ${own.length}${from && from !== "steam" ? ` · from ${from}` : ""}`}>
          <Trophies list={own} prefix="ach" />
        </Row>
      )}
      {milestones.length > 0 && (
        <Row title={`CarlaOS milestones · ${earned(milestones)} of ${milestones.length}`}>
          <Trophies list={milestones} prefix="mile" milestone />
        </Row>
      )}
      {details && (
        <section className="row">
          <h2 className="row__title">Saves</h2>
          <SavesPanel game={game} saves={details.saves} sync={details.sync} onSaves={(saves) => setDetails({ ...details, saves })} notify={ctx.notify} />
        </section>
      )}
    </div>
  );
}
