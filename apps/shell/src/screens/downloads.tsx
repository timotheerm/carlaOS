// Everything the stores are fetching, here and on the desktops, with the
// step each one is at. Anyone may look; nothing here changes anything.
import type { Download, Game, Host } from "../data/types";
import { Ring, StateLine, Steps, installState, placeName, speed, stateOfDownload, timeLeft } from "../ui/install";
import { Cover, Empty, Progress, size } from "../ui/parts";

/** One sentence on how far a download is: sizes, speed and time left. */
export function howFar(d: Download): string {
  const parts = [d.total > 0 ? `${size(d.done)} of ${size(d.total)}` : "Steam is working out how much there is", speed(d), timeLeft(d)];
  return parts.filter(Boolean).join(" · ");
}

export function Downloads({ library, downloads, hosts, openGame }: { library: Game[]; downloads: Download[]; hosts: Host[]; openGame: (id: string) => void }) {
  // This Ally first, then the desktops; within each, the furthest along first.
  const order = (d: Download) => (d.place === "local" ? 0 : 1) * 1000 - (d.total ? d.done / d.total : -1);
  const list = [...downloads].sort((a, b) => order(a) - order(b));
  const fetching = new Set(downloads.map((d) => d.gameId));
  const waiting = library.filter((g) => (g.updates?.length ?? 0) > 0 && !fetching.has(g.id));
  return (
    <div className="downloads">
      <div className="pagehead">
        <h1>Downloads</h1>
        <p>{list.length ? "What is being installed or updated, and the step each one is at. Steam does the downloading." : "Nothing is being installed or updated right now."}</p>
      </div>
      {list.length === 0 && waiting.length === 0 && <Empty>Games of your library that are not installed are under My Games, then Not installed.</Empty>}
      {list.map((d, i) => {
        const game = library.find((g) => g.id === d.gameId);
        const state = stateOfDownload(d);
        return (
          <button
            key={`${d.gameId}:${d.place}`}
            className="download"
            data-focus
            data-autofocus={i === 0 ? "" : undefined}
            data-fid={`dl:${d.gameId}:${d.place}`}
            data-game={d.gameId}
            onClick={() => openGame(d.gameId)}
            aria-label={`${d.title}, on ${placeName(d.place, hosts)}, ${state.label}, ${howFar(d)}`}
          >
            <span className="download__cover">{game ? <Cover game={game} band={false} /> : null}</span>
            <span className="download__body">
              <span className="download__title">{d.title}</span>
              <span className="download__where">
                {d.update ? "Updating" : "Installing"} on {d.place === "local" ? "this Ally" : placeName(d.place, hosts)}
              </span>
              <Steps download={d} />
              <Progress download={d} />
              <span className="download__how">{howFar(d)}</span>
            </span>
            <Ring state={state} />
          </button>
        );
      })}
      {waiting.length > 0 && (
        <>
          <h2 className="row__title downloads__title">Waiting for an update</h2>
          {waiting.map((g, i) => (
            <button key={g.id} className="download download--waiting" data-focus data-autofocus={list.length === 0 && i === 0 ? "" : undefined} data-fid={`dl:wait:${g.id}`} data-game={g.id} onClick={() => openGame(g.id)}>
              <span className="download__cover">
                <Cover game={g} band={false} />
              </span>
              <span className="download__body">
                <span className="download__title">{g.title}</span>
                <span className="download__where">
                  {(g.updates ?? []).map((u) => (u.place === "local" ? "On this Ally" : `On ${placeName(u.place, hosts)}`)).join(" and ")}
                </span>
                <StateLine state={installState(g, [])} extra="starting the game begins the update" />
              </span>
            </button>
          ))}
        </>
      )}
    </div>
  );
}
