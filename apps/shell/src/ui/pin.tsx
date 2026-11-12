// The administrator PIN pad. Whoever shows it decides what unlocking does.
import { useEffect, useRef, useState } from "react";
import type { UnlockResult } from "../cleanup/types";

const PIN_LENGTH = 4;

export function PinPad({
  pinSet,
  unlock,
  setPin,
  lede,
}: {
  pinSet: boolean;
  unlock: (pin: string) => Promise<UnlockResult>;
  setPin: (pin: string) => Promise<void>;
  /** Why this part is locked, in one sentence. */
  lede: string;
}) {
  const [digits, setDigits] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  // Refs, so a key press always sees the latest entry whoever handles it.
  const entry = useRef("");
  const first = useRef<string | null>(null); // first entry when choosing a PIN
  const checking = useRef(false);

  const show = (value: string) => {
    entry.current = value;
    setDigits(value);
  };

  const submit = async (pin: string) => {
    let problem: string | null = null;
    if (pinSet) {
      checking.current = true;
      try {
        const r = await unlock(pin);
        if (r.result === "unlocked") return; // this pad is about to disappear
        problem = r.result === "wait" ? `Too many tries. Wait ${r.seconds} seconds.` : "That is not the PIN.";
      } catch {
        problem = "The PIN could not be checked.";
      } finally {
        checking.current = false;
      }
    } else if (first.current === null) {
      first.current = pin;
      setConfirming(true);
    } else if (first.current === pin) {
      try {
        await setPin(pin);
        return;
      } catch {
        problem = "The PIN could not be saved.";
      }
    } else {
      first.current = null;
      setConfirming(false);
      problem = "Those did not match. Start again.";
    }
    show("");
    setNote(problem);
  };

  const press = (d: string) => {
    if (checking.current || entry.current.length >= PIN_LENGTH) return;
    setNote(null);
    const next = entry.current + d;
    show(next);
    if (next.length === PIN_LENGTH) void submit(next);
  };
  const erase = () => {
    if (!checking.current) show(entry.current.slice(0, -1));
  };

  // A keyboard can type the PIN directly.
  const pressRef = useRef(press);
  pressRef.current = press;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (/^[0-9]$/.test(e.key)) pressRef.current(e.key);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const title = pinSet ? "Administrator PIN" : confirming ? "Enter it again" : "Choose an administrator PIN";

  return (
    <div className="pin">
      <h1>{title}</h1>
      <p className="lede">{pinSet ? lede : "Four digits. It keeps this part of CarlaOS away from whoever is just playing."}</p>
      <div className="pin__dots" role="status" aria-label={`${digits.length} of ${PIN_LENGTH} digits entered`}>
        {Array.from({ length: PIN_LENGTH }, (_, n) => (
          <i key={n} className={n < digits.length ? "on" : undefined} />
        ))}
      </div>
      <p className="pin__note" role="alert">
        {note ?? " "}
      </p>
      <div className="pin__pad">
        {["1", "2", "3", "4", "5", "6", "7", "8", "9"].map((d) => (
          <button key={d} className="pin__key" data-focus data-autofocus={d === "5" ? "" : undefined} data-fid={`pin:${d}`} onClick={() => press(d)}>
            {d}
          </button>
        ))}
        <span />
        <button className="pin__key" data-focus data-fid="pin:0" onClick={() => press("0")}>
          0
        </button>
        <button className="pin__key" data-focus data-fid="pin:del" aria-label="Delete" onClick={erase}>
          ⌫
        </button>
      </div>
    </div>
  );
}

// Six digits read off the desktop's screen. Same pad as the PIN, but the
// digits stay visible: there is nothing to hide and they are easy to mistype.
export function CodePad({
  length,
  title,
  lede,
  note,
  busy,
  onComplete,
}: {
  length: number;
  title: string;
  lede: string;
  /** What went wrong with the last code, if anything. */
  note: string | null;
  /** The last code is being checked; the pad waits. */
  busy: boolean;
  onComplete: (code: string) => void;
}) {
  const [digits, setDigits] = useState("");
  const entry = useRef("");
  const busyRef = useRef(busy);
  busyRef.current = busy;
  const show = (value: string) => {
    entry.current = value;
    setDigits(value);
  };
  // A refused code clears the pad for the next try.
  useEffect(() => {
    if (!busy) show("");
  }, [busy, note]);

  const press = (d: string) => {
    if (busyRef.current || entry.current.length >= length) return;
    const next = entry.current + d;
    show(next);
    if (next.length === length) onComplete(next);
  };
  const pressRef = useRef(press);
  pressRef.current = press;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (/^[0-9]$/.test(e.key)) pressRef.current(e.key);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="pin">
      <h1>{title}</h1>
      <p className="lede">{lede}</p>
      <div className="pin__code" role="status" aria-label={`${digits.length} of ${length} digits entered`}>
        {Array.from({ length }, (_, n) => (
          <i key={n} className={n === 3 ? "gap" : undefined}>
            {digits[n] ?? ""}
          </i>
        ))}
      </div>
      <p className="pin__note" role="alert">
        {busy ? "Linking…" : digits ? " " : (note ?? " ")}
      </p>
      <div className="pin__pad">
        {["1", "2", "3", "4", "5", "6", "7", "8", "9"].map((d) => (
          <button key={d} className="pin__key" data-focus data-autofocus={d === "5" ? "" : undefined} data-fid={`code:${d}`} onClick={() => press(d)}>
            {d}
          </button>
        ))}
        <span />
        <button className="pin__key" data-focus data-fid="code:0" onClick={() => press("0")}>
          0
        </button>
        <button className="pin__key" data-focus data-fid="code:del" aria-label="Delete" onClick={() => !busyRef.current && show(entry.current.slice(0, -1))}>
          ⌫
        </button>
      </div>
    </div>
  );
}
