// Administrator page for desktops: link one with the code its companion shows,
// or stop using one. The player never needs this page.
import { useEffect, useLayoutEffect, useState } from "react";
import { backend } from "../cleanup/backend";
import type { AdminStatus } from "../cleanup/types";
import type { CheckFix, CheckReport, FoundHost, Host, HostPicture } from "../data/types";
import { library, message } from "../library/store";
import { focusFirst, keepFocus } from "../nav/focus";
import { CodePad, PinPad } from "../ui/pin";

const PICTURES: Record<string, string> = {
  auto: "Automatic",
  auto120: "Automatic, up to 120 a second",
  smooth: "Always 1080p at 120, 40 Mbit/s",
  sharp: "Always 1080p at 60, 25 Mbit/s",
  balanced: "Always 1080p at 60, 12 Mbit/s",
  light: "Always 720p at 60, 7 Mbit/s",
  lightest: "Always 720p at 30, 3 Mbit/s",
};

function lastPicture(last: NonNullable<HostPicture["last"]>): string {
  const got = `Last stream: ${last.height}p at ${last.fps}, ${last.mbps} Mbit/s`;
  if (!last.automatic || last.latency_ms === null) return got;
  const speed = last.link_kbps === null ? "" : last.link_kbps >= 10_000 ? `${Math.round(last.link_kbps / 1000)} Mbit/s, ` : `${(last.link_kbps / 1000).toFixed(1)} Mbit/s, `;
  return `${got} · connection ${speed}${last.latency_ms} ms${last.away ? ", away from home" : ""}`;
}

// The picture of streams from this desktop. Automatic measures the
// connection before each stream; each press steps to the next choice.
function PictureSetting({ host, notify }: { host: Host; notify: (text: string) => void }) {
  const [picture, setPicture] = useState<HostPicture | null>(null);
  useEffect(() => {
    let current = true;
    library.hostPicture(host.id).then((p) => current && setPicture(p), () => undefined);
    return () => {
      current = false;
    };
  }, [host.id]);
  const note = picture?.last ? lastPicture(picture.last) : picture?.setting.startsWith("auto") ? "The connection is measured before each stream." : null;
  return (
    <>
      <button
        className="setting"
        data-focus
        data-fid="desk:picture"
        onClick={() => {
          if (!picture) return;
          const next = picture.settings[(picture.settings.indexOf(picture.setting) + 1) % picture.settings.length];
          library.setHostPicture(host.id, next).then(setPicture, (e) => notify(message(e)));
        }}
      >
        <span>Picture</span>
        <span className="setting__value">{picture === null ? "…" : (PICTURES[picture.setting] ?? picture.setting)}</span>
      </button>
      {note && <p className="setting__note">{note}</p>}
    </>
  );
}

type Step =
  | { at: "list" }
  | { at: "searching" }
  | { at: "found"; found: FoundHost[] }
  | { at: "address" }
  | { at: "code"; address: string; name: string | null }
  | { at: "done"; id: string; name: string; games: number; warning: string | null }
  | { at: "check"; id: string }
  | { at: "host"; id: string }
  | { at: "remove"; host: Host };

// Whether this desktop may administer the Ally: add games, clean up Windows,
// update. Off until the administrator turns it on here.
function AdminSwitch({ host, notify }: { host: Host; notify: (text: string) => void }) {
  const [allowed, setAllowed] = useState<boolean | null>(null);
  useEffect(() => {
    let current = true;
    library.hostAdmin(host.id).then((a) => current && setAllowed(a), () => current && setAllowed(false));
    return () => {
      current = false;
    };
  }, [host.id]);
  return (
    <button
      className="setting"
      data-focus
      data-fid="desk:admin"
      onClick={() => {
        if (allowed === null) return;
        const next = !allowed;
        library.setHostAdmin(host.id, next).then(
          () => {
            setAllowed(next);
            notify(next ? `${host.name} may now administer this Ally.` : `${host.name} may no longer administer this Ally.`);
          },
          (e) => notify(message(e)),
        );
      }}
    >
      <span>Administer this Ally from {host.name}</span>
      <span className="setting__value">
        {allowed === null ? "…" : allowed ? "Allowed · commands typed there are carried out here" : "Not allowed"}
      </span>
    </button>
  );
}

const MARK: Record<CheckReport["steps"][number]["outcome"], string> = { ok: "✓", warn: "!", fail: "✕", skipped: "–" };
const FIX_WORDS: Record<CheckFix, string> = {
  pair: "Pair streaming again",
  wake: "Wake it",
  update: "Update the desktop",
  relink: "Link it again with a new code",
};
const FIX_WORKING: Record<CheckFix, string> = {
  pair: "Pairing Moonlight with Sunshine there…",
  wake: "Waking it, this can take a minute…",
  update: "Asking it to update…",
  relink: "",
};

// Every link in the chain a stream needs, in order, with the one press that
// repairs a broken one when CarlaOS can do that by itself.
function CheckDesktop({ host, relink, done }: { host: Host; relink: () => void; done: () => void }) {
  const [report, setReport] = useState<CheckReport | null>(null);
  const [working, setWorking] = useState<string | null>("Checking…");
  const [problem, setProblem] = useState<string | null>(null);

  const run = (job: Promise<CheckReport>, doing: string) => {
    setWorking(doing);
    setProblem(null);
    job.then(setReport, (e) => setProblem(message(e))).finally(() => setWorking(null));
  };
  useEffect(() => run(library.checkHost(host.id), "Checking…"), [host.id]);
  useLayoutEffect(() => {
    const main = document.querySelector("main");
    if (main && !working) focusFirst(main);
  }, [working]);

  const fixes = report
    ? [...new Set(report.steps.filter((s) => s.fix && (s.outcome === "fail" || s.outcome === "warn")).map((s) => s.fix as CheckFix))]
    : [];
  const worst = report?.steps.some((s) => s.outcome === "fail") ? "fail" : report?.steps.some((s) => s.outcome === "warn") ? "warn" : "ok";
  const summary = !report
    ? "Each thing a stream needs, in order."
    : worst === "ok"
      ? "Everything a stream needs is in place."
      : worst === "warn"
        ? "Streams work. A few things are worth knowing."
        : fixes.length
          ? "Streams cannot start yet. CarlaOS can fix it from here."
          : "Streams cannot start yet. The line marked ✕ says what to do.";

  return (
    <div className="settings">
      <div className="pagehead">
        <h1>Check {host.name}</h1>
        <p>{summary}</p>
      </div>
      {working && (
        <p className="working" role="status">
          {working}
        </p>
      )}
      {problem && (
        <div className="notice notice--warn" role="alert">
          <strong>{problem}</strong>
        </div>
      )}
      {report && !working && (
        <>
          {report.repaired.length > 0 && (
            <div className="notice" role="status">
              {report.repaired.map((r) => (
                <strong key={r}>{r}</strong>
              ))}
            </div>
          )}
          <ol className="checks">
            {report.steps.map((s) => (
              <li key={s.key} className={`check check--${s.outcome}`}>
                <span className="check__mark" aria-hidden="true">
                  {MARK[s.outcome]}
                </span>
                <span className="check__title">{s.title}</span>
                <span className="check__detail">{s.detail}</span>
              </li>
            ))}
          </ol>
          {fixes.map((fix, i) => (
            <button
              key={fix}
              className="setting setting--primary"
              data-focus
              data-autofocus={i === 0 ? "" : undefined}
              data-fid={`check:${fix}`}
              onClick={() => (fix === "relink" ? relink() : run(library.repairHost(host.id, fix), FIX_WORKING[fix]))}
            >
              <span>{FIX_WORDS[fix]}</span>
              <span className="setting__value">{fix === "relink" ? "Needs the code the desktop shows" : "Then checks again"}</span>
            </button>
          ))}
        </>
      )}
      {!working && (
        <>
          <button className="setting" data-focus data-autofocus={fixes.length ? undefined : ""} data-fid="check:again" onClick={() => run(library.checkHost(host.id), "Checking…")}>
            <span>Check again</span>
            <span className="setting__value" />
          </button>
          <button className="setting" data-focus data-fid="check:done" onClick={done}>
            <span>Done</span>
            <span className="setting__value">Back to {host.name}</span>
          </button>
        </>
      )}
    </div>
  );
}

export function LinkDesktops({ hosts, notify }: { hosts: Host[]; notify: (text: string) => void }) {
  const [admin, setAdmin] = useState<AdminStatus | null>(null);
  const [step, setStep] = useState<Step>({ at: "list" });
  const [address, setAddress] = useState("");
  const [linking, setLinking] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Locked on the way in and again on the way out.
  useEffect(() => {
    void backend.status().then(setAdmin, () => setAdmin({ pin_set: false, unlocked: false }));
    return () => void backend.lock().catch(() => undefined);
  }, []);

  useLayoutEffect(() => {
    const main = document.querySelector("main");
    if (main) focusFirst(main);
  }, [admin?.unlocked, step.at, hosts.length]);

  // "Put to sleep" leaves the page when the desktop stops answering.
  const watched = step.at === "host" ? hosts.find((h) => h.id === step.id)?.state : null;
  useLayoutEffect(() => keepFocus("desk:rescan"), [watched]);

  if (!admin) return null;
  if (!admin.unlocked) {
    return (
      <PinPad
        pinSet={admin.pin_set}
        lede="Linking and removing desktops is locked."
        notThis={
          admin.pin_set
            ? "This is the Ally's own four-digit PIN, not the code from the desktop. The six-digit code is asked for next."
            : "This PIN belongs to the Ally and is yours to choose. It is not the code from the desktop: that six-digit code is asked for next."
        }
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
        lede="Type the six digits the CarlaOS companion shows on the desktop."
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
        {step.warning && (
          <button className="setting setting--primary" data-focus data-autofocus data-fid="link:check" onClick={() => setStep({ at: "check", id: step.id })}>
            <span>Check {step.name} and fix it</span>
            <span className="setting__value">Pairs streaming again without a new code</span>
          </button>
        )}
        <button className="setting" data-focus data-autofocus={step.warning ? undefined : ""} data-fid="link:ok" onClick={() => setStep({ at: "list" })}>
          <span>Done</span>
          <span className="setting__value" />
        </button>
      </div>
    );
  }

  if (step.at === "check") {
    const host = hosts.find((h) => h.id === step.id);
    if (!host) {
      return (
        <div className="settings">
          <button className="setting" data-focus data-autofocus data-fid="check:gone" onClick={() => setStep({ at: "list" })}>
            <span>This desktop is no longer set up</span>
            <span className="setting__value">Back</span>
          </button>
        </div>
      );
    }
    return (
      <CheckDesktop
        host={host}
        relink={() => {
          setProblem(null);
          setStep({ at: "code", address: host.address ?? "", name: host.name });
        }}
        done={() => setStep({ at: "host", id: host.id })}
      />
    );
  }

  if (step.at === "host") {
    const host = hosts.find((h) => h.id === step.id);
    if (!host) {
      return (
        <div className="settings">
          <button className="setting" data-focus data-autofocus data-fid="desk:back" onClick={() => setStep({ at: "list" })}>
            <span>This desktop is no longer set up</span>
            <span className="setting__value">Back</span>
          </button>
        </div>
      );
    }
    const awake = host.state === "ready" || host.state === "busy" || host.state === "not_ready";
    return (
      <div className="settings">
        <div className="pagehead">
          <h1>{host.name}</h1>
          <p>
            {host.hardware || "Desktop"}
            {host.address ? ` · ${host.address}` : ""}
          </p>
        </div>
        <button
          className="setting"
          data-focus
          data-autofocus
          data-fid="desk:rescan"
          onClick={() => {
            if (busy) return;
            setBusy(true);
            library.rescanHost(host.id).then(
              (games) => notify(`${games} ${games === 1 ? "game" : "games"} on ${host.name}.`),
              (e) => notify(message(e)),
            ).finally(() => setBusy(false));
          }}
        >
          <span>Look for games again</span>
          <span className="setting__value">{busy ? "Looking…" : "After installing or adding a game there"}</span>
        </button>
        <button className="setting" data-focus data-fid="desk:check" onClick={() => setStep({ at: "check", id: host.id })}>
          <span>Check streaming from it</span>
          <span className="setting__value">Finds what is wrong and fixes it</span>
        </button>
        {awake && (
          <button
            className="setting"
            data-focus
            data-fid="desk:sleep"
            onClick={() =>
              library.sleepHost(host.id).then(
                () => notify(`${host.name} is going to sleep.`),
                (e) => notify(message(e)),
              )
            }
          >
            <span>Put to sleep</span>
            <span className="setting__value">{host.canWake ? "The Ally can wake it again" : "It has not said how to wake it yet"}</span>
          </button>
        )}
        <PictureSetting host={host} notify={notify} />
        <AdminSwitch host={host} notify={notify} />
        <button className="setting" data-focus data-fid="desk:remove" onClick={() => setStep({ at: "remove", host })}>
          <span>Remove {host.name}</span>
          <span className="setting__value">Stops streaming from it</span>
        </button>
        <button className="setting" data-focus data-fid="desk:back" onClick={() => setStep({ at: "list" })}>
          <span>Done</span>
          <span className="setting__value">Back to Desktops</span>
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
        <button className="setting" data-focus data-autofocus data-fid="unlink:no" onClick={() => setStep({ at: "host", id: host.id })}>
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
        <button key={h.id} className="setting" data-focus data-fid={`desk:${h.id}`} onClick={() => setStep({ at: "host", id: h.id })}>
          <span>{h.name}</span>
          <span className="setting__value">{h.address ? `${h.address} · manage` : "Manage"}</span>
        </button>
      ))}
    </div>
  );
}
