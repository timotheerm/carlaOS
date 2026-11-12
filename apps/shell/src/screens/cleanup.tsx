// Administrator-only screens for cleaning up Windows. The home page is a short
// menu of big rows; the lists live on their own pages so a controller never has
// to travel through forty rows to reach a button.
import { useEffect, useLayoutEffect } from "react";
import { cleanup, useCleanup, type CleanupState } from "../cleanup/store";
import type { Item, Kind, Outcome, Tier } from "../cleanup/types";
import { focusFirst } from "../nav/focus";
import { PinPad } from "../ui/pin";

export type CleanupView = "home" | "review" | "changes" | "kept";

const KIND_LABEL: Record<Kind, string> = {
  app: "App",
  startup: "Starts with Windows",
  service: "Background service",
  setting: "Windows setting",
};

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

// Content changes under the same page (unlocking, a scan finishing) can remove
// the focused element. Put focus back on something so A always does something.
function useKeepFocus(marker: unknown) {
  useLayoutEffect(() => {
    const active = document.activeElement;
    if (active instanceof HTMLElement && active.matches("[data-focus]")) return;
    const main = document.querySelector("main");
    if (main) focusFirst(main);
  }, [marker]);
}

function Result({ last }: { last: NonNullable<CleanupState["last"]> }) {
  const { done, failed, restart }: Outcome = last.outcome;
  if (done.length === 0 && failed.length === 0) return null;
  return (
    <div className={failed.length ? "notice notice--warn" : "notice"} role="status">
      <strong>
        {plural(done.length, "change")} {last.verb}
        {failed.length ? `, ${failed.length} not` : ""}.
        {restart && done.length ? " Restart the Ally to finish." : ""}
      </strong>
      {failed.map((f) => (
        <span key={f.id}>
          {f.title}: {f.error}
        </span>
      ))}
    </div>
  );
}

function Home({ state, open }: { state: CleanupState; open: (view: CleanupView) => void }) {
  const { report, selected, busy, last, error } = state;
  const count = (tier: Tier) => report?.items.filter((i) => i.tier === tier).length ?? 0;
  const approvals = report?.items.filter((i) => selected.has(i.id) && i.needs_admin).length ?? 0;
  const offered = count("recommended") + count("optional");
  useKeepFocus(`${busy}:${!!report}:${selected.size}`);

  return (
    <div className="settings">
      <div className="pagehead">
        <h1>Clean up Windows</h1>
      </div>
      <p className="lede">
        Everything here can be undone. ASUS, Xbox, security, driver and controller parts are never touched.
      </p>
      {last && <Result last={last} />}
      {error && (
        <div className="notice notice--warn" role="alert">
          <strong>{error}</strong>
        </div>
      )}
      {report?.warnings.map((w) => (
        <div key={w} className="notice notice--warn">
          <span>{w}</span>
        </div>
      ))}

      {busy && (
        <p className="working" role="status">
          {busy === "scanning"
            ? "Looking at this Ally…"
            : busy === "applying"
              ? "Applying. If Windows asks for approval, choose Yes."
              : "Undoing. If Windows asks for approval, choose Yes."}
        </p>
      )}

      {!busy && report && (
        <>
          {selected.size > 0 ? (
            <button className="setting setting--primary" data-focus data-autofocus data-fid="clean:apply" onClick={() => void cleanup.apply()}>
              <span>Apply {plural(selected.size, "change")}</span>
              <span className="setting__value">
                {approvals ? "Windows will ask for approval once" : "No approval needed"}
              </span>
            </button>
          ) : (
            <p className="lede">{offered ? "Nothing is selected." : "Nothing left to clean up. This Ally is tidy."}</p>
          )}
          {offered + count("manual") > 0 && (
            <button className="setting" data-focus data-fid="clean:review" onClick={() => open("review")}>
              <span>Review the list</span>
              <span className="setting__value">
                {count("recommended")} recommended · {count("optional")} your call
                {count("manual") ? ` · ${count("manual")} by hand` : ""}
              </span>
            </button>
          )}
          {report.changes.length > 0 && (
            <button className="setting" data-focus data-fid="clean:changes" onClick={() => open("changes")}>
              <span>Changes made</span>
              <span className="setting__value">{report.changes.length} in effect · undo any of them</span>
            </button>
          )}
          <button className="setting" data-focus data-fid="clean:kept" onClick={() => open("kept")}>
            <span>Kept on purpose</span>
            <span className="setting__value">{plural(count("kept"), "thing")} CarlaOS never touches</span>
          </button>
        </>
      )}
      {!busy && (
        <button className="setting" data-focus data-fid="clean:scan" onClick={() => void cleanup.scan()}>
          <span>{report ? "Scan again" : "Scan this Ally"}</span>
          <span className="setting__value">Only looks, changes nothing</span>
        </button>
      )}
    </div>
  );
}

function Row({ item, checked, onClick, action }: { item: Item; checked?: boolean; onClick?: () => void; action?: string }) {
  return (
    <button
      className="tune"
      data-focus
      data-fid={`tune:${item.id}`}
      role={checked === undefined ? undefined : "checkbox"}
      aria-checked={checked}
      onClick={onClick}
    >
      {checked !== undefined && <i className={checked ? "tune__box tune__box--on" : "tune__box"} aria-hidden />}
      <span className="tune__text">
        <span className="tune__title">{item.title}</span>
        {item.detail && <span className="tune__detail">{item.detail}</span>}
      </span>
      <span className="tune__tags">
        <span className="tag">{KIND_LABEL[item.kind]}</span>
        {item.needs_admin && checked !== undefined && <span className="tag tag--quiet">Needs approval</span>}
        {action && <span className="tag tag--action">{action}</span>}
      </span>
    </button>
  );
}

function Section({ title, note, children }: { title: string; note: string; children: React.ReactNode }) {
  return (
    <section className="tunes">
      <h2 className="row__title">{title}</h2>
      <p className="lede">{note}</p>
      {children}
    </section>
  );
}

function Review({ state }: { state: CleanupState }) {
  const items = state.report?.items ?? [];
  const of = (tier: Tier) => items.filter((i) => i.tier === tier);
  const toggles = (list: Item[]) =>
    list.map((item) => (
      <Row key={item.id} item={item} checked={state.selected.has(item.id)} onClick={() => cleanup.toggle(item.id)} />
    ));
  return (
    <>
      <div className="pagehead">
        <h1>{plural(state.selected.size, "change")} selected</h1>
        <p>Press B to go back and apply.</p>
      </div>
      <div className="chips chips--below">
        <button className="chip" data-focus data-autofocus data-fid="tune:recommended" onClick={() => cleanup.select("recommended")}>
          Recommended only
        </button>
        <button className="chip" data-focus data-fid="tune:none" onClick={() => cleanup.select("none")}>
          Clear all
        </button>
      </div>
      {of("recommended").length > 0 && (
        <Section title="Recommended" note="No downside on a handheld that boots into CarlaOS.">
          {toggles(of("recommended"))}
        </Section>
      )}
      {of("optional").length > 0 && (
        <Section title="Your call" note="These help, but each costs something. Read the line before ticking it.">
          {toggles(of("optional"))}
        </Section>
      )}
      {of("manual").length > 0 && (
        <Section title="For you to do by hand" note="CarlaOS could not undo these, so it does not do them.">
          {of("manual").map((item) => (
            <Row key={item.id} item={item} />
          ))}
        </Section>
      )}
    </>
  );
}

function Changes({ state }: { state: CleanupState }) {
  const changes = state.report?.changes ?? [];
  useKeepFocus(`${state.busy}:${changes.length}`);
  if (state.busy) {
    return (
      <p className="working" role="status">
        Undoing. If Windows asks for approval, choose Yes.
      </p>
    );
  }
  return (
    <>
      <div className="pagehead">
        <h1>Changes made</h1>
        {changes.length > 0 && (
          <button className="pill" data-focus data-autofocus data-fid="undo:all" onClick={() => void cleanup.undo(changes.map((c) => c.item_id))}>
            Undo everything
          </button>
        )}
      </div>
      {state.last && <Result last={state.last} />}
      {changes.length === 0 ? (
        <p className="empty">Nothing is changed right now.</p>
      ) : (
        <section className="tunes">
          {changes.map((c) => (
            <Row
              key={c.item_id}
              item={{
                id: c.item_id,
                kind: c.kind,
                tier: "recommended",
                title: c.title,
                detail: `Changed on ${new Date(c.applied_at * 1000).toLocaleDateString([], { day: "numeric", month: "long", year: "numeric" })}${c.needs_admin ? ". Undoing needs approval." : "."}`,
                needs_admin: c.needs_admin,
              }}
              action="Undo"
              onClick={() => void cleanup.undo([c.item_id])}
            />
          ))}
        </section>
      )}
    </>
  );
}

function Kept({ state }: { state: CleanupState }) {
  const kept = (state.report?.items ?? []).filter((i) => i.tier === "kept");
  const reasons = [...new Set(kept.map((i) => i.detail))];
  return (
    <>
      <div className="pagehead">
        <h1>Kept on purpose</h1>
        <p>CarlaOS never changes these.</p>
      </div>
      {reasons.map((reason) => (
        <section key={reason} className="tunes">
          <p className="lede">{reason}</p>
          {kept
            .filter((i) => i.detail === reason)
            .map((item) => (
              <Row key={item.id} item={{ ...item, detail: "" }} />
            ))}
        </section>
      ))}
    </>
  );
}

export function Cleanup({ view, open }: { view: CleanupView; open: (view: CleanupView) => void }) {
  const state = useCleanup();
  useEffect(() => {
    void cleanup.open();
  }, []);
  useKeepFocus(state.admin?.unlocked);

  if (!state.admin) return null;
  if (!state.admin.unlocked) {
    return (
      <PinPad
        pinSet={state.admin.pin_set}
        unlock={cleanup.unlock}
        setPin={cleanup.setPin}
        lede="This part changes Windows, so it is locked."
      />
    );
  }
  if (view === "review") return <Review state={state} />;
  if (view === "changes") return <Changes state={state} />;
  if (view === "kept") return <Kept state={state} />;
  return <Home state={state} open={open} />;
}
