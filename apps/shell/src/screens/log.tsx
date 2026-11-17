// The activity log: what the connection to the desktops was doing, and the crash trail, by time.
// Shown to be read and copied for whoever looks after CarlaOS. Nothing in it identifies a desktop
// by its address, and it holds no keys or PINs.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { message } from "../library/store";
import { activity } from "../native";
import { focusFirst } from "../nav/focus";

const LINES = 200;
const EVERY = 3_000;

export function ActivityLog() {
  const [lines, setLines] = useState<string[] | null>(null);
  const [copied, setCopied] = useState(false);
  const [plain, setPlain] = useState("");
  const [problem, setProblem] = useState("");
  const box = useRef<HTMLPreElement>(null);
  // Whether the reader is at the bottom: only then do new lines pull the view down.
  const atBottom = useRef(true);
  const copiedTimer = useRef<ReturnType<typeof setTimeout>>(undefined);

  // Fresh while the screen is open.
  useEffect(() => {
    let alive = true;
    const look = () =>
      activity.tail(LINES).then(
        (l) => alive && setLines(l),
        (e) => alive && (setProblem(message(e)), setLines((old) => old ?? [])),
      );
    void look();
    const timer = setInterval(() => void look(), EVERY);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);

  useLayoutEffect(() => {
    const main = document.querySelector("main");
    if (main && lines) focusFirst(main);
  }, [lines !== null]);

  useEffect(() => () => clearTimeout(copiedTimer.current), []);

  // The newest lines are the ones that matter: stay at the bottom, unless the reader scrolled up.
  useLayoutEffect(() => {
    if (box.current && atBottom.current) box.current.scrollTop = box.current.scrollHeight;
  }, [lines]);

  const onScroll = () => {
    const b = box.current;
    if (b) atBottom.current = b.scrollHeight - b.scrollTop - b.clientHeight < 24;
  };
  // A page up or down, for the controller.
  const page = (dir: -1 | 1) => box.current?.scrollBy({ top: dir * box.current.clientHeight * 0.8 });

  const copy = async () => {
    setProblem("");
    let text = "";
    try {
      text = await activity.text();
      await navigator.clipboard.writeText(text);
      setCopied(true);
      clearTimeout(copiedTimer.current);
      copiedTimer.current = setTimeout(() => setCopied(false), 2000);
    } catch {
      // No clipboard here: show the text to select.
      setPlain(text || (lines ?? []).join("\n"));
    }
  };

  if (!lines) return null;
  return (
    <div className="settings">
      <div className="pagehead">
        <h1>Activity log</h1>
        <p>What the connection to the desktops was doing. Copy it and send it to whoever looks after CarlaOS.</p>
      </div>
      {problem && (
        <div className="notice notice--warn">
          <b>{problem}</b>
        </div>
      )}
      {lines.length === 0 ? <p className="lede">Nothing has been written yet.</p> : <pre className="activitylog" ref={box} onScroll={onScroll}>{lines.join("\n")}</pre>}
      <button className="setting" data-focus data-fid="log:older" onClick={() => page(-1)}>
        <span>Older lines</span>
        <span className="setting__value">Scroll the log up</span>
      </button>
      <button className="setting" data-focus data-fid="log:newer" onClick={() => page(1)}>
        <span>Newer lines</span>
        <span className="setting__value">Scroll the log down</span>
      </button>
      <button className="setting" data-focus data-autofocus data-fid="log:copy" onClick={() => void copy()}>
        <span>Copy log</span>
        <span className="setting__value">{copied ? "Copied" : "Everything kept, newest last"}</span>
      </button>
      {plain && <textarea readOnly value={plain} rows={8} onFocus={(e) => e.currentTarget.select()} />}
      <button className="setting" data-focus data-fid="log:folder" onClick={() => activity.openFolder().catch((e) => setProblem(message(e)))}>
        <span>Open folder</span>
        <span className="setting__value">Shows the log files in Explorer</span>
      </button>
    </div>
  );
}
