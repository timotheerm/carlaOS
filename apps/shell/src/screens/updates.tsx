// Administrator page for updates: bring this machine, and the linked
// desktops, to the newest CarlaOS release without fetching installers by hand.
import { useCallback, useEffect, useLayoutEffect, useState } from "react";
import { backend } from "../cleanup/backend";
import type { AdminStatus } from "../cleanup/types";
import type { Host } from "../data/types";
import { message } from "../library/store";
import { focusFirst } from "../nav/focus";
import { PinPad } from "../ui/pin";
import { older, updates, type Progress, type Updates as Found } from "../updates/backend";

type Step = { at: "looking" } | { at: "list" } | { at: "problem"; text: string } | { at: "installing"; progress: Progress | null } | { at: "restarting" };

export function Updates({ hosts, notify, refreshHosts }: { hosts: Host[]; notify: (text: string) => void; refreshHosts: () => void }) {
  const [admin, setAdmin] = useState<AdminStatus | null>(null);
  const [found, setFound] = useState<Found | null>(null);
  const [step, setStep] = useState<Step>({ at: "looking" });
  // Desktops that were just told to update, and the version they are moving to.
  const [moving, setMoving] = useState<Record<string, string>>({});

  // Locked on the way in and again on the way out.
  useEffect(() => {
    void backend.status().then(setAdmin, () => setAdmin({ pin_set: false, unlocked: false }));
    return () => void backend.lock().catch(() => undefined);
  }, []);

  const look = useCallback(async () => {
    setStep({ at: "looking" });
    try {
      setFound(await updates.check());
      setStep({ at: "list" });
    } catch (e) {
      setStep({ at: "problem", text: message(e) });
    }
  }, []);

  const unlocked = admin?.unlocked === true;
  useEffect(() => {
    if (unlocked) void look();
  }, [unlocked, look]);

  useLayoutEffect(() => {
    const main = document.querySelector("main");
    if (main) focusFirst(main);
  }, [unlocked, step.at]);

  if (!admin) return null;
  if (!admin.unlocked) {
    return (
      <PinPad
        pinSet={admin.pin_set}
        lede="Updating CarlaOS is locked."
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

  if (step.at === "looking") {
    return (
      <p className="working" role="status">
        Looking for a newer CarlaOS…
      </p>
    );
  }

  if (step.at === "installing") {
    const p = step.progress;
    const percent = p && p.total > 0 ? Math.min(100, Math.round((p.received / p.total) * 100)) : null;
    return (
      <p className="working" role="status">
        {percent === null ? "Getting the update…" : `Getting the update… ${percent}%`}
      </p>
    );
  }

  if (step.at === "restarting") {
    return (
      <p className="working" role="status">
        Installing. CarlaOS closes for a moment and opens again by itself.
      </p>
    );
  }

  if (step.at === "problem" || !found) {
    return (
      <div className="settings">
        <div className="pagehead">
          <h1>Updates</h1>
          <p>{step.at === "problem" ? step.text : "Nothing is known yet."}</p>
        </div>
        <button className="setting" data-focus data-autofocus data-fid="upd:again" onClick={() => void look()}>
          <span>Look again</span>
          <span className="setting__value" />
        </button>
      </div>
    );
  }

  const hostHere = found.host_here;
  const newest = found.available?.version ?? hostHere?.available?.version ?? found.current;
  const here = found.available !== null || (hostHere?.available ?? null) !== null;

  const install = async () => {
    setStep({ at: "installing", progress: null });
    try {
      const changed = await updates.install((progress) => setStep({ at: "installing", progress }));
      if (!changed) {
        notify("This is already the newest version.");
        await look();
      } else if (found.available) {
        // The interface is being replaced: it closes on its own.
        setStep({ at: "restarting" });
      } else {
        notify("The desktop host on this machine is updating.");
        await look();
      }
    } catch (e) {
      setStep({ at: "problem", text: message(e) });
    }
  };

  const updateHost = async (host: Host) => {
    notify(`Asking ${host.name}…`);
    try {
      const answer = await updates.updateHost(host.id);
      if (answer.updating) {
        setMoving((m) => ({ ...m, [host.id]: answer.version }));
        notify(`${host.name} is updating to ${answer.version}. It is away for a moment.`);
        setTimeout(refreshHosts, 20_000);
      } else {
        notify(`${host.name} already has the newest version.`);
        refreshHosts();
      }
    } catch (e) {
      notify(message(e));
    }
  };

  const hostValue = (host: Host) => {
    if (moving[host.id] && host.version !== moving[host.id]) return `Updating to ${moving[host.id]}…`;
    if (!host.version) return host.state === "unknown" ? "Checking…" : "Not answering · turn it on to update it";
    return older(host.version, newest) ? `${host.version} · update to ${newest}` : `${host.version} · up to date`;
  };

  return (
    <div className="settings">
      <div className="pagehead">
        <h1>Updates</h1>
        <p>
          {here
            ? `CarlaOS ${newest} is out. Updating keeps the library, settings and linked desktops.`
            : `This machine has the newest CarlaOS, ${found.current}.`}
        </p>
      </div>
      {here && (
        <button className="setting setting--primary" data-focus data-autofocus data-fid="upd:install" onClick={() => void install()}>
          <span>Update this machine</span>
          <span className="setting__value">To {newest} · CarlaOS reopens by itself</span>
        </button>
      )}
      <div className="setting" data-fid="upd:ally">
        <span>CarlaOS on this machine</span>
        <span className="setting__value">{found.available ? `${found.current} · ${found.available.version} is out` : `${found.current} · up to date`}</span>
      </div>
      {hostHere && (
        <div className="setting" data-fid="upd:hosthere">
          <span>Desktop host on this machine</span>
          <span className="setting__value">
            {hostHere.available ? `${hostHere.current} · ${hostHere.available.version} is out` : `${hostHere.current} · up to date`}
          </span>
        </div>
      )}
      {hosts.map((host) => {
        const can = !!host.version && older(host.version, newest) && !moving[host.id];
        return (
          <button
            key={host.id}
            className="setting"
            data-focus
            data-fid={`upd:host:${host.id}`}
            onClick={() => (can ? void updateHost(host) : notify(hostValue(host)))}
          >
            <span>{host.name}</span>
            <span className="setting__value">{hostValue(host)}</span>
          </button>
        );
      })}
      <button className="setting" data-focus data-autofocus={here ? undefined : ""} data-fid="upd:again" onClick={() => void look()}>
        <span>Look again</span>
        <span className="setting__value">Asks GitHub for the newest release</span>
      </button>
    </div>
  );
}
