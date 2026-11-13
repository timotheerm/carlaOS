// Administrator page: whether CarlaOS starts when the Ally is turned on, and
// the ways back to the Windows desktop.
import { useEffect, useLayoutEffect, useState } from "react";
import { backend } from "../cleanup/backend";
import type { AdminStatus } from "../cleanup/types";
import { message } from "../library/store";
import { boot } from "../native";
import { focusFirst } from "../nav/focus";
import { PinPad } from "../ui/pin";

export function Startup({ notify }: { notify: (text: string) => void }) {
  const [admin, setAdmin] = useState<AdminStatus | null>(null);
  const [enabled, setEnabled] = useState<boolean | null>(null);

  // Locked on the way in and again on the way out.
  useEffect(() => {
    void backend.status().then(setAdmin, () => setAdmin({ pin_set: false, unlocked: false }));
    void boot.ready().then((s) => setEnabled(s.enabled), () => setEnabled(false));
    return () => void backend.lock().catch(() => undefined);
  }, []);

  const unlocked = admin?.unlocked === true;
  useLayoutEffect(() => {
    const main = document.querySelector("main");
    if (main) focusFirst(main);
  }, [unlocked]);

  if (!admin) return null;
  if (!admin.unlocked) {
    return (
      <PinPad
        pinSet={admin.pin_set}
        lede="How the Ally starts is locked."
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

  return (
    <div className="settings">
      <div className="pagehead">
        <h1>Starting up</h1>
        <p>{enabled ? "The Ally opens CarlaOS by itself when it is turned on." : "The Ally opens the Windows desktop when it is turned on."}</p>
      </div>
      <button
        className="setting"
        data-focus
        data-autofocus
        data-fid="start:toggle"
        onClick={() => {
          if (enabled === null) return;
          boot.set(!enabled).then(
            (now) => {
              setEnabled(now);
              notify(now ? "CarlaOS starts with the Ally from now on." : "CarlaOS no longer starts by itself.");
            },
            (e) => notify(message(e)),
          );
        }}
      >
        <span>Start CarlaOS when the Ally turns on</span>
        <span className="setting__value">{enabled === null ? "…" : enabled ? "On" : "Off"}</span>
      </button>
      <div className="notice">
        <b>Ways back to the Windows desktop</b>
        <span>Settings, then Exit to Windows, at any time.</span>
        <span>Hold View (the button with two squares) while the Ally starts, to skip CarlaOS that once.</span>
        <span>If CarlaOS fails to start twice in a row, it stays out of the way the next time by itself.</span>
      </div>
    </div>
  );
}
