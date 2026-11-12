// Administrator page for desktops: link one with the code its companion shows,
// or stop using one. The player never needs this page.
import { useEffect, useLayoutEffect, useState } from "react";
import { backend } from "../cleanup/backend";
import type { AdminStatus } from "../cleanup/types";
import type { FoundHost, Host } from "../data/types";
import { library, message } from "../library/store";
import { focusFirst } from "../nav/focus";
import { CodePad, PinPad } from "../ui/pin";

type Step =
  | { at: "list" }
  | { at: "searching" }
  | { at: "found"; found: FoundHost[] }
  | { at: "address" }
  | { at: "code"; address: string; name: string | null }
  | { at: "done"; name: string; games: number; warning: string | null }
  | { at: "remove"; host: Host };

export function LinkDesktops({ hosts, notify }: { hosts: Host[]; notify: (text: string) => void }) {
  const [admin, setAdmin] = useState<AdminStatus | null>(null);
  const [step, setStep] = useState<Step>({ at: "list" });
  const [address, setAddress] = useState("");
  const [linking, setLinking] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  // Locked on the way in and again on the way out.
  useEffect(() => {
    void backend.status().then(setAdmin, () => setAdmin({ pin_set: false, unlocked: false }));
    return () => void backend.lock().catch(() => undefined);
  }, []);

  useLayoutEffect(() => {
    const main = document.querySelector("main");
    if (main) focusFirst(main);
  }, [admin?.unlocked, step.at, hosts.length]);

  if (!admin) return null;
  if (!admin.unlocked) {
    return (
      <PinPad
        pinSet={admin.pin_set}
        lede="Linking and removing desktops is locked."
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

  const search = async () => {
    setStep({ at: "searching" });
    const found = await library.discover().catch(() => []);
    setStep({ at: "found", found });
  };

  const link = async (to: string, code: string) => {
    setLinking(true);
    setProblem(null);
    try {
      setStep({ at: "done", ...(await library.link(to, code)) });
    } catch (e) {
      setProblem(message(e));
    } finally {
      setLinking(false);
    }
  };

  if (step.at === "searching") {
    return (
      <p className="working" role="status">
        Looking for desktops on this network…
      </p>
    );
  }

  if (step.at === "found") {
    return (
      <div className="settings">
        <div className="pagehead">
          <h1>Add a desktop</h1>
          <p>
            {step.found.length
              ? "Choose the desktop, then enter the code it shows."
              : "No desktop answered. Start the CarlaOS companion on the desktop and look again."}
          </p>
        </div>
        {step.found.map((f, i) => (
          <button
            key={f.id}
            className="setting"
            data-focus
            data-autofocus={i === 0 ? "" : undefined}
            data-fid={`link:${f.id}`}
            onClick={() => {
              setProblem(null);
              setStep({ at: "code", address: f.address, name: f.name });
            }}
          >
            <span>{f.name}</span>
            <span className="setting__value">
              {hosts.some((h) => h.id === f.id) ? "Linked already · link again" : f.address}
            </span>
          </button>
        ))}
        <button className="setting" data-focus data-autofocus={step.found.length ? undefined : ""} data-fid="link:again" onClick={() => void search()}>
          <span>Look again</span>
          <span className="setting__value">Desktops on this network</span>
        </button>
        <button className="setting" data-focus data-fid="link:address" onClick={() => setStep({ at: "address" })}>
          <span>Enter an address</span>
          <span className="setting__value">For a desktop that is not found</span>
        </button>
        <button className="setting" data-focus data-fid="link:cancel" onClick={() => setStep({ at: "list" })}>
          <span>Cancel</span>
          <span className="setting__value" />
        </button>
      </div>
    );
  }

  if (step.at === "address") {
    const usable = address.trim().length > 0;
    return (
      <div className="form">
        <div className="pagehead">
          <h1>Enter an address</h1>
          <p>The desktop's name or IP address on the network.</p>
        </div>
        <label className="form__row">
          <span>Address</span>
          <input
            className="field"
            data-focus
            data-autofocus
            data-fid="link:field"
            value={address}
            maxLength={120}
            placeholder="192.168.1.20"
            onChange={(e) => setAddress(e.target.value)}
          />
        </label>
        <div className="form__row">
          <span />
          <div className="form__actions">
            <button
              className="pill pill--primary"
              data-focus
              data-fid="link:next"
              disabled={!usable}
              onClick={() => {
                setProblem(null);
                setStep({ at: "code", address: address.trim(), name: null });
              }}
            >
              Continue
            </button>
            <button className="pill" data-focus data-fid="link:back" onClick={() => void search()}>
              Cancel
            </button>
          </div>
        </div>
      </div>
    );
  }

  if (step.at === "code") {
    return (
      <CodePad
        length={6}
        title={`Code from ${step.name ?? step.address}`}
        lede="Open the CarlaOS companion on the desktop. It shows six digits."
        note={problem}
        busy={linking}
        onComplete={(code) => void link(step.address, code)}
      />
    );
  }

  if (step.at === "done") {
    return (
      <div className="settings">
        <div className="pagehead">
          <h1>{step.name} is linked</h1>
          <p>
            {step.warning
              ? "Its games are listed, but streaming is not ready yet."
              : `${step.games} ${step.games === 1 ? "game" : "games"} can now be streamed from it.`}
          </p>
        </div>
        {step.warning && (
          <div className="notice notice--warn" role="alert">
            <strong>{step.warning}</strong>
          </div>
        )}
        <button className="setting" data-focus data-autofocus data-fid="link:ok" onClick={() => setStep({ at: "list" })}>
          <span>Done</span>
          <span className="setting__value" />
        </button>
      </div>
    );
  }

  if (step.at === "remove") {
    const host = step.host;
    return (
      <div className="settings">
        <div className="pagehead">
          <h1>Remove {host.name}?</h1>
          <p>Its games leave the library unless they are also somewhere else. Playtime is kept. You can link it again later.</p>
        </div>
        <button className="setting" data-focus data-autofocus data-fid="unlink:no" onClick={() => setStep({ at: "list" })}>
          <span>Keep it</span>
          <span className="setting__value" />
        </button>
        <button
          className="setting"
          data-focus
          data-fid="unlink:yes"
          onClick={() =>
            library.removeHost(host.id).then(
              () => {
                notify(`${host.name} was removed.`);
                setStep({ at: "list" });
              },
              (e) => notify(message(e)),
            )
          }
        >
          <span>Remove {host.name}</span>
          <span className="setting__value">Stops streaming from it</span>
        </button>
      </div>
    );
  }

  return (
    <div className="settings">
      <div className="pagehead">
        <h1>Desktops</h1>
        <p>Computers this Ally can stream games from. Each one runs the CarlaOS companion.</p>
      </div>
      <button className="setting" data-focus data-autofocus data-fid="link:add" onClick={() => void search()}>
        <span>Add a desktop</span>
        <span className="setting__value">Needs the code it shows</span>
      </button>
      {hosts.map((h) => (
        <button key={h.id} className="setting" data-focus data-fid={`unlink:${h.id}`} onClick={() => setStep({ at: "remove", host: h })}>
          <span>{h.name}</span>
          <span className="setting__value">{h.address ? `${h.address} · remove` : "Remove"}</span>
        </button>
      ))}
    </div>
  );
}
