// Run by cargo test (crates/companion/src/dashboard.rs) when node is installed.
// Runs the dashboard page's own h() and offered() with a tiny fake DOM and simulates a click
// whose event.currentTarget becomes null once the handler returned (as in a browser).
import fs from 'fs';
import vm from 'vm';

const file = process.argv[2] || new URL('../crates/companion/src/dashboard.html', import.meta.url);
const html = fs.readFileSync(file, 'utf8').split('\r\n').join('\n');

function grab(startMarker, endMarker) {
  const a = html.indexOf(startMarker);
  const b = html.indexOf(endMarker, a);
  if (a < 0 || b < 0) throw new Error('marker not found: ' + startMarker);
  return html.slice(a, b);
}
const hSrc = grab('function h(tag, attrs, ...children) {', 'function svg(');
const offeredSrc = grab('function offered(list) {', '$("find-button").addEventListener');

class El {
  constructor(tag) { this.tag = tag; this.children = []; this.handlers = {}; this.parent = null; this.removed = false; this.attrs = {}; }
  addEventListener(name, fn) { this.handlers[name] = fn; }
  setAttribute(k, v) { this.attrs[k] = v; }
  append(...cs) { for (const c of cs) { if (c instanceof El) c.parent = this; this.children.push(c); } }
  replaceChildren(...cs) { this.children = []; this.append(...cs); }
  closest(sel) { let n = this; while (n) { if (n.tag === sel) return n; n = n.parent; } return null; }
  remove() { this.removed = true; }
  find(tag) { for (const c of this.children) { if (!(c instanceof El)) continue; if (c.tag === tag) return c; const r = c.find(tag); if (r) return r; } return null; }
}
const found = new El('ul');
const toasts = [];
const context = {
  document: { createElement: (t) => new El(t), getElementById: () => found },
  toast: (m) => toasts.push(m),
  act: async (button, method, path, body, done) => { await Promise.resolve(); done({ title: 'Test Game' }); },
  Object, Array, JSON, Promise,
};
context.$ = (id) => found;
vm.createContext(context);
vm.runInContext(hSrc + '\n' + offeredSrc + '\nglobalThis.h = h; globalThis.offered = offered;', context);

context.offered([{ id: 'abc', title: 'Test Game', exe: 'D:\\Games\\x.exe', note: 'The main program', watch: null }]);
const li = found.children[0];
const button = li.find('button');
const event = { currentTarget: button };
let thrown = null;
const unhandled = [];
process.on('unhandledRejection', (e) => unhandled.push(e));
try { button.handlers.click(event); } catch (e) { thrown = e; }
event.currentTarget = null; // the browser clears it once dispatch is over
await new Promise((r) => setTimeout(r, 20));

const problems = [];
if (thrown) problems.push('click handler threw: ' + thrown.message);
if (unhandled.length) problems.push('unhandled: ' + unhandled[0]);
if (!li.removed) problems.push('the row was not removed after adding');
if (!toasts.some((t) => t.includes('Test Game was added'))) problems.push('no confirmation toast: ' + JSON.stringify(toasts));
console.log(problems.length ? 'FAIL: ' + problems.join('; ') : 'PASS: add click removes the row and confirms');
process.exit(problems.length ? 1 : 0);
