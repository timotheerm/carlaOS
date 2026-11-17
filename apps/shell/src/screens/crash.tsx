// Crash reports kept on this Ally, so one can be sent to whoever looks after CarlaOS.
import { useEffect, useLayoutEffect, useState } from "react";
import { message } from "../library/store";
import { crash, type ReportInfo } from "../native";
import { focusFirst } from "../nav/focus";

const KIND = { panic: "Crashed", unexpected: "Closed unexpectedly" } as const;

function when(at: number): string {
  return new Date(at * 1000).toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
}

export function Crash() {
  const [reports, setReports] = useState<ReportInfo[] | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [copied, setCopied] = useState(false);
  const [plain, setPlain] = useState(false);
  const [problem, setProblem] = useState("");

  useEffect(() => {
    void crash.reports().then(setReports, () => setReports([]));
    void crash.seen().catch(() => undefined);
  }, []);

  useLayoutEffect(() => {
    const main = document.querySelector("main");
    if (main && reports) focusFirst(main);
  }, [reports !== null]);

  const choose = (name: string) => {
    setOpen(name);
    setText("");
    setCopied(false);
    setPlain(false);
    setProblem("");
    crash.text(name).then(setText, (e) => setProblem(message(e)));
  };

  const copy = () =>
    navigator.clipboard.writeText(text).then(
      () => {
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
      },
      () => setPlain(true),
    );

  if (!reports) return null;
  return (
    <div className="settings">
      <div className="pagehead">
        <h1>Crash reports</h1>
        <p>Send a copy of a report to whoever looks after CarlaOS.</p>
      </div>
      {reports.length === 0 && <p className="lede">No crashes recorded.</p>}
      {reports.map((r, i) => (
        <button
          key={r.name}
          className="setting"
          data-focus
          data-autofocus={i === 0 ? true : undefined}
          data-fid={`crash:${r.name}`}
          onClick={() => choose(r.name)}
        >
          <span>{KIND[r.kind] ?? "Report"}</span>
          <span className="setting__value">
            {r.new ? "New · " : ""}
            {when(r.at)}
          </span>
        </button>
      ))}
      {open && (
        <div className="notice">
          <b style={{ whiteSpace: "pre-wrap" }}>{problem || text.split("\n").slice(0, 12).join("\n")}</b>
        </div>
      )}
      {open && text && (
        <button className="setting" data-focus data-fid="crash:copy" onClick={copy}>
          <span>Copy report</span>
          <span className="setting__value">{copied ? "Copied" : ""}</span>
        </button>
      )}
      {plain && <textarea readOnly value={text} rows={8} onFocus={(e) => e.currentTarget.select()} />}
      <button className="setting" data-focus data-fid="crash:folder" onClick={() => crash.openFolder().catch((e) => setProblem(message(e)))}>
        <span>Open folder</span>
        <span className="setting__value">Shows the reports in Explorer</span>
      </button>
    </div>
  );
}
