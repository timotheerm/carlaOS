import type { Session } from "../data/types";

// Shown instead of the library while a game is running. The game normally
// covers it; it is what the player sees on the way in, on the way out, and if
// they come back to CarlaOS while the game is still open.
export function Playing({ session, onEnd }: { session: Session; onEnd: () => void }) {
  return (
    <div className="playing">
      <p className="hero__eyebrow">{session.phase === "starting" ? "Starting" : "Playing"}</p>
      <h1 className="detail__title">{session.title}</h1>
      <p className="lede">
        {session.phase === "starting"
          ? "This can take a moment the first time."
          : "CarlaOS comes back by itself when you quit the game."}
      </p>
      <button className="pill" data-focus data-autofocus data-fid="playing:end" onClick={onEnd}>
        I have finished playing
      </button>
    </div>
  );
}
