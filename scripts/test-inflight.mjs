// Tests apps/shell/src/inflight.ts with a fake window: a slow command is noted once while it runs and
// once when it ends; fast, ignored and failing commands behave. Run: node scripts/test-inflight.mjs
import { stripTypeScriptTypes } from 'node:module';
import fs from 'fs';
import vm from 'vm';

const src = fs.readFileSync(new URL('../apps/shell/src/inflight.ts', import.meta.url), 'utf8');
// Node strips the types itself; the ES imports/exports become plain functions for the fake page.
const js = stripTypeScriptTypes(src)
  .replace(/^import \{ crash, inTauri \} from "\.\/native";$/m, 'const { crash, inTauri } = require("./native");')
  .replace(/^export function/m, 'function')
  + '\nmodule.exports = { watchCommands };';

const notes = [];
let now = 0;
const timers = [];
const pending = new Map();
let nextId = 1;
const win = {
  setTimeout: (fn, ms) => { const id = nextId++; timers.push({ id, at: now + ms, fn }); return id; },
  clearTimeout: (id) => { const i = timers.findIndex((t) => t.id === id); if (i >= 0) timers.splice(i, 1); },
};
win.__TAURI_INTERNALS__ = {
  invoke: (cmd) => new Promise((resolve, reject) => pending.set(cmd, { resolve, reject })),
};
const module = { exports: {} };
const fakeNative = { crash: { note: async (t) => { notes.push(t); } }, inTauri: true };
const context = {
  module, exports: module.exports, window: win, Date: { now: () => now }, Math, Promise, void: undefined,
  require: (m) => (m === './native' ? fakeNative : (() => { throw new Error(m); })()),
};
vm.createContext(context);
vm.runInContext(js, context);
module.exports.watchCommands();

const tick = async (ms) => {
  now += ms;
  for (const t of timers.filter((x) => x.at <= now)) { timers.splice(timers.indexOf(t), 1); t.fn(); }
  await Promise.resolve(); await Promise.resolve();
};
const inv = (cmd) => win.__TAURI_INTERNALS__.invoke(cmd);
const check = (name, ok) => { if (!ok) { console.log('FAIL ' + name, JSON.stringify(notes)); process.exit(1); } console.log('ok   ' + name); };

// 1. a command that takes 12 s is noted while it runs and when it ends
const slow = inv('hosts_refresh');
await tick(8000);
check('a stuck command is noted once while it runs', notes.length === 1 && notes[0] === 'command hosts_refresh still running after 8 s');
await tick(4000);
pending.get('hosts_refresh').resolve(1);
await slow; await tick(0);
check('and again when it ends', notes.length === 2 && notes[1] === 'command hosts_refresh finished after 12 s');

// 2. a quick command writes nothing and leaves no timer behind
notes.length = 0;
const quick = inv('library_list');
await tick(1000);
pending.get('library_list').resolve(1);
await quick; await tick(20000);
check('a quick command is silent', notes.length === 0 && timers.length === 0);

// 3. ignored commands are passed through untouched
const ignored = inv('crash_note');
await tick(60000);
pending.get('crash_note').resolve(1);
await ignored;
check('ignored commands are never watched', notes.length === 0);

// 4. a failing stuck command is noted as failed, and the error still reaches the caller
const bad = inv('hosts_wake');
await tick(9000);
pending.get('hosts_wake').reject(new Error('nope'));
let caught = null;
await bad.catch((e) => { caught = e; });
await tick(0);
check('a failing command reaches its caller and is noted', caught && caught.message === 'nope' && notes.at(-1) === 'command hosts_wake failed after 9 s');
// 5. installing must never throw, whatever the page's internals look like (a white screen otherwise)
for (const [name, shape] of [
  ['a frozen object', () => Object.freeze({ invoke: () => Promise.resolve(1) })],
  ['a read-only invoke', () => Object.defineProperty({}, 'invoke', { value: () => Promise.resolve(1), writable: false })],
  ['no invoke at all', () => ({})],
  ['no internals', () => undefined],
]) {
  win.__TAURI_INTERNALS__ = shape();
  let threw = null;
  try { module.exports.watchCommands(); } catch (e) { threw = e; }
  check(`installing on ${name} does not throw`, threw === null);
}
// 6. a call that returns something that is not a promise is handed back untouched
win.__TAURI_INTERNALS__ = { invoke: () => 42 };
module.exports.watchCommands();
check('a non-promise result is passed through', win.__TAURI_INTERNALS__.invoke('x') === 42);
console.log('all passed');
