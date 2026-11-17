import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { QuickMenu } from "./screens/quick";
import { crash } from "./native";
import { watchCommands } from "./inflight";
import "./tokens.css";
import "./app.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {/* The same program serves two windows: the console, and the menu over a game. */}
    {location.hash === "#quick" ? <QuickMenu /> : <App />}
  </StrictMode>,
);

// Problems the page itself runs into go to the trail: the message only.
function note(kind: string, what: unknown) {
  try {
    const raw = what instanceof Error ? what.message : typeof what === "string" ? what : "";
    const text = raw.replace(/https?:\/\/[^\s?#]+[^\s]*/g, (u) => u.split(/[?#]/)[0]).slice(0, 200);
    void crash.note(`${kind}: ${text}`);
  } catch {
    /* never throw from here */
  }
}
window.addEventListener("error", (e) => note("error", e.message));
window.addEventListener("unhandledrejection", (e) => note("unhandled rejection", e.reason));
// A heartbeat, so a window that stops answering is reported (see docs/CRASH-REPORTS.md).
// It says whether the page is hidden, so a covered window is not mistaken for a frozen one.
const beat = () => void crash.alive(document.hidden);
beat();
window.setInterval(beat, 3000);
document.addEventListener("visibilitychange", beat);

// Once the first screen is up (never before: a diagnostic must not be able to stop the page from
// starting), a command the page waits on for long is written down.
window.setTimeout(() => watchCommands(), 2000);
