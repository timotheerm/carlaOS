// Remove one selection from the personal library. Local Steam installs are
// uninstalled through Steam's own confirmation and only then removed.
import { useEffect, useState } from "react";
import { backend } from "../cleanup/backend";
import type { AdminStatus } from "../cleanup/types";
import type { Game } from "../data/types";
import { library, message } from "../library/store";
import { PinPad } from "../ui/pin";

export function RemoveGame({ game, onDone, notify }: { game: Game; onDone: () => void; notify: (text: string) => void }) {
  const localSteam = game.platform === "Steam" && game.id.startsWith("steam:") && game.places.includes("local");
  const [admin, setAdmin] = useState<AdminStatus | null>(null);
  const [statusError, setStatusError] = useState(false);
  const [checking, setChecking] = useState(localSteam);
  const [sure, setSure] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!localSteam) return;
    let current = true;
    backend.status().then(
      (status) => { if (current) setAdmin(status); },
      () => { if (current) setStatusError(true); },
    ).finally(() => { if (current) setChecking(false); });
    return () => {
      current = false;
      void backend.lock().catch(() => undefined);
    };
  }, [localSteam]);

  const retryStatus = () => {
    setChecking(true);
    setStatusError(false);
    setAdmin(null);
    void backend.status().then(
      setAdmin,
      () => setStatusError(true),
    ).finally(() => setChecking(false));
  };

  const removeSelection = async () => {
    setBusy(true);
    try {
      await library.removeFromMyLibrary(game.id);
      notify(game.title + " was removed from My Library. Its installation was not changed.");
      onDone();
    } catch (e) {
      notify(message(e));
      setBusy(false);
    }
  };

  const askSteam = async () => {
    setBusy(true);
    try {
      await library.removeAndUninstallFromMyLibrary(game.id);
      notify("Steam is asking whether to uninstall " + game.title + ". Confirm there to remove it from My Library. Cancel to keep it.");
      onDone();
    } catch (e) {
      notify(message(e));
      setBusy(false);
    }
  };

  if (localSteam) {
    if (checking) {
      return (
        <div className="settings remove-status">
          <div className="pagehead">
            <h1>Check administrator access</h1>
            <p>Before Steam can uninstall this game.</p>
          </div>
          <div className="notice">
            <b>Checking access…</b>
            <span>CarlaOS is checking whether the administrator PIN is set and unlocked.</span>
          </div>
        </div>
      );
    }
    if (statusError || !admin) {
      return (
        <div className="settings remove-status">
          <div className="pagehead">
            <h1>Cannot check administrator access</h1>
            <p>No changes have been made to this game.</p>
          </div>
          <div className="notice notice--warn">
            <b>CarlaOS could not check the administrator PIN.</b>
            <span>Check that the shell is responding, then try again.</span>
          </div>
          <button className="setting setting--primary" data-focus data-autofocus data-fid="remove:retry" onClick={retryStatus}>
            <span>Try again</span>
            <span className="setting__value">Check administrator access</span>
          </button>
          <button className="setting" data-focus data-fid="remove:back" onClick={onDone}>
            <span>Keep in My Library</span>
            <span className="setting__value">Nothing will be changed</span>
          </button>
        </div>
      );
    }
    if (!admin.unlocked) {
      return (
        <PinPad
          pinSet={admin.pin_set}
          lede="Uninstalling a game from My Library needs the administrator PIN. Steam will ask for its own confirmation too."
          unlock={async (pin) => {
            const result = await backend.unlock(pin);
            if (result.result === "unlocked") setAdmin({ pin_set: true, unlocked: true });
            return result;
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
          <h1>Remove {game.title}</h1>
          <p>This game is installed on the Ally through Steam.</p>
        </div>
        <p className="lede">CarlaOS will ask Steam to uninstall the local copy. Steam shows its own confirmation, and nothing is removed unless you confirm there.</p>
        <p className="lede">If you cancel Steam's prompt, the game stays installed and remains in My Library.</p>
        <button className="setting setting--primary" data-focus data-autofocus data-fid="remove:steam" disabled={busy} onClick={() => void askSteam()}>
          <span>Ask Steam to uninstall</span>
          <span className="setting__value">{busy ? "Waiting for Steam…" : "Steam confirmation required"}</span>
        </button>
        <button className="setting" data-focus data-fid="remove:back" disabled={busy} onClick={onDone}>
          <span>Keep in My Library</span>
          <span className="setting__value">Nothing will be changed</span>
        </button>
      </div>
    );
  }

  const remote = game.places.some((place) => place !== "local" && place !== "cloud");
  const localNonSteam = game.places.includes("local");
  const detail = remote
    ? "This only removes the game from this Ally's My Library. It stays installed on the desktop and can still be streamed after you add it again."
    : localNonSteam
      ? "CarlaOS does not uninstall this kind of local game. The files and emulator configuration stay on the Ally; use the original store or app to uninstall them."
      : "This only removes the selection from My Library. The game remains available in its original store or on its desktop.";

  return (
    <div className="settings">
      <div className="pagehead">
        <h1>Remove {game.title}</h1>
        <p>Remove this selection from My Library?</p>
      </div>
      <p className="lede">{detail}</p>
      <button
        className={sure ? "setting setting--primary" : "setting"}
        data-focus
        data-autofocus
        data-fid="remove:confirm"
        disabled={busy}
        onClick={() => {
          if (!sure) {
            setSure(true);
            return;
          }
          void removeSelection();
        }}
      >
        <span>{sure ? "Press again to remove" : "Remove from My Library"}</span>
        <span className="setting__value">{busy ? "Removing…" : "Does not delete game files"}</span>
      </button>
      <button className="setting" data-focus data-fid="remove:cancel" disabled={busy} onClick={onDone}>
        <span>Keep in My Library</span>
        <span className="setting__value">Go back</span>
      </button>
    </div>
  );
}
