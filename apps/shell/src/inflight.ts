// Writes down which native command the interface is waiting on when it waits too long.
//
// A frozen Ally left no trace of what it was stuck on. Every call from the page to the Rust side
// goes through `__TAURI_INTERNALS__.invoke`; wrapping it here covers all of them. A call still
// running after STUCK_AFTER_MS is noted once ("still running"), and again when it finally ends
// ("finished after N s"). Only the command's name and the time are written: never its arguments.
import { crash, inTauri } from "./native";

const STUCK_AFTER_MS = 8000;
// Slow by design (they wait for a person or a long job), and the calls that write the notes themselves.
const IGNORED = /^(crash_|log_|hosts_link_wait$|hosts_discover$|library_scan$|update_|installs_start$|plugin:)/;

type Invoke = (cmd: string, args?: unknown, options?: unknown) => Promise<unknown>;

export function watchCommands(): void {
  if (!inTauri) return;
  const internals = (window as unknown as { __TAURI_INTERNALS__?: { invoke?: Invoke; __carlaWatched?: boolean } }).__TAURI_INTERNALS__;
  if (!internals?.invoke || internals.__carlaWatched) return;
  const original = internals.invoke.bind(internals);
  internals.__carlaWatched = true;
  internals.invoke = (cmd, args, options) => {
    if (IGNORED.test(cmd)) return original(cmd, args, options);
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
    const call = original(cmd, args, options);
    call.then(
      () => done("finished"),
      () => done("failed"),
    );
    return call;
  };
}
