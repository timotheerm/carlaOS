import type { Game, Session } from "../data/types";
import { Cover } from "../ui/parts";

// Shown instead of the library while a game is running. The game normally
// covers it; it is what the player sees on the way in, on the way out, and if
// they come back to CarlaOS while the game is still open.
export function Playing({ session, game, from, onEnd }: { session: Session; game?: Game; from?: string; onEnd: () => void }) {
  const waiting = session.phase !== "running";
  return (
    <div className={waiting ? "playing playing--waiting" : "playing"}>
      {game && (
        <div className="playing__cover">
          <Cover game={game} />
        </div>
      )}
      <p className="hero__eyebrow">{session.phase === "waking" ? "Waking the desktop" : session.phase === "starting" ? "Starting" : "Playing"}</p>
      <h1 className="detail__title">{session.title}</h1>
      <p className="lede">
        {session.phase === "waking"
          ? "It was asleep. This can take up to a minute."
          : session.phase === "starting"
          ? session.cloud
            ? "Opening it. The first time, sign in to the service."
            : session.stream
            ? `Connecting to ${from ?? "the desktop"}.`
            : "This can take a moment the first time."
          : session.cloud
            ? "CarlaOS comes back by itself when it is closed."
            : session.stream
            ? "CarlaOS comes back by itself when the stream ends."
            : "CarlaOS comes back by itself when you quit the game."}
      </p>
      {waiting && (
        <span className="playing__dots" aria-hidden>
          <i />
          <i />
          <i />
        </span>
      )}
      {session.phase === "running" && <p className="lede">Hold View and Menu together for sound, brightness and ending the game.</p>}
      <button className="pill" data-focus data-autofocus data-quiet data-fid="playing:end" onClick={onEnd}>
        {session.phase === "running" ? "I have finished playing" : "Stop"}
      </button>
    </div>
  );
}
