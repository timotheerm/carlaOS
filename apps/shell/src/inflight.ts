// Writes down which native command the interface is waiting on when it waits too long.
//
// A frozen Ally left no trace of what it was stuck on. Every call from the page to the Rust side
// goes through `__TAURI_INTERNALS__.invoke`; wrapping it here covers all of them. A call still
// running after STUCK_AFTER_MS is noted once ("still running"), and again when it finally ends
// ("finished after N s"). Only the command's name and the time are written: never its arguments.
//
// This is a diagnostic and must never get in the way: it is installed after the first screen is up,
// any failure while installing it leaves the page untouched, and the wrapper hands every call to the
// original function whatever happens inside the watching.
import { crash, inTauri } from "./native";

const STUCK_AFTER_MS = 8000;
// Slow by design (they wait for a person or a long job), and the calls that write the notes themselves.
const IGNORED = /^(crash_|log_|hosts_link_wait$|hosts_discover$|library_scan$|update_|installs_start$|plugin:)/;

type Invoke = (cmd: string, args?: unknown, options?: unknown) => Promise<unknown>;

export function watchCommands(): void {
  try {
    if (!inTauri) return;
    const internals = (window as unknown as { __TAURI_INTERNALS__?: { invoke?: Invoke } }).__TAURI_INTERNALS__;
    if (!internals || typeof internals.invoke !== "function") return;
    const original = internals.invoke;
    const watched: Invoke = function (cmd, args, options) {
      const call = original.call(internals, cmd, args, options);
      try {
        if (typeof cmd !== "string" || IGNORED.test(cmd) || !call || typeof (call as Promise<unknown>).then !== "function") return call;
        const began = Date.now();
        let stuck = false;
        const timer = window.setTimeout(() => {
          stuck = true;
          void crash.note(`command ${cmd} still running after ${Math.round(STUCK_AFTER_MS / 1000)} s`);
        }, STUCK_AFTER_MS);
        const done = (how: string) => {
          window.clearTimeout(timer);
          if (stuck) void crash.note(`command ${cmd} ${how} after ${Math.round((Date.now() - began) / 1000)} s`);
        };
        // A separate branch: the caller still gets the original promise, with its own result or error.
        (call as Promise<unknown>).then(
          () => done("finished"),
          () => done("failed"),
        );
      } catch {
        /* the watching must never break a call */
      }
      return call;
    };
    // If the property cannot be replaced, nothing is watched and nothing breaks.
    internals.invoke = watched;
  } catch {
    /* a diagnostic: never allowed to stop the page */
  }
}
