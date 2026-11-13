#!/usr/bin/env node
// Turn what the build produced into the files people download:
//
//   CarlaOS-Ally-<version>-setup.exe          the interface, for the Ally
//   CarlaOS-Desktop-Host-<version>-setup.exe  the companion, for a desktop PC
//   CarlaOS-Full-<version>-setup.exe          both, with a tick box for each
//   SHA256SUMS.txt
//
//   node package.mjs --version 0.5.0 --source <repository> --out <folder> [--no-host]
//
// --source is the checkout that was built (its target/ folder holds the
// results). --no-host makes only the Ally installer, for versions from before
// the desktop host existed. Needs NSIS: set MAKENSIS, or have makensis on PATH.
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

function fail(message) {
  console.error(message);
  process.exit(1);
}

const args = process.argv.slice(2);
function option(name) {
  const at = args.indexOf(`--${name}`);
  return at >= 0 ? args[at + 1] : undefined;
}

const version = option("version") ?? "";
if (!/^\d+\.\d+\.\d+$/.test(version)) fail("Give --version such as 0.5.0 (three numbers, nothing else).");
const source = resolve(option("source") ?? join(here, ".."));
const out = resolve(option("out") ?? join(source, "target", "installers"));
const withHost = !args.includes("--no-host");
const makensis = process.env.MAKENSIS || "makensis";

// NSIS on Windows only finds files given with backslashes, which is what
// Node's paths already are there. Elsewhere paths have none to begin with.
const nsisPath = (path) => path;

function nsis(script, defines) {
  const flags = Object.entries(defines).map(([name, value]) => `-D${name}=${value}`);
  const run = spawnSync(makensis, ["-V2", ...flags, join(here, script)], { stdio: "inherit" });
  if (run.error) fail(`NSIS could not be started (${makensis}): ${run.error.message}`);
  if (run.status !== 0) fail(`NSIS could not build ${script}.`);
}

mkdirSync(out, { recursive: true });
const made = [];

// 1. The Ally installer is Tauri's, under a name that says where it goes.
const bundle = join(source, "target", "release", "bundle", "nsis");
const built = existsSync(bundle) ? readdirSync(bundle).filter((name) => name.toLowerCase().endsWith(".exe")) : [];
if (built.length !== 1) fail(`Expected exactly one installer in ${bundle}, found ${built.length}. Run "npx tauri build" in apps/shell first.`);
if (!built[0].includes(version)) fail(`${built[0]} is not version ${version}. Stamp the version before building.`);
const ally = join(out, `CarlaOS-Ally-${version}-setup.exe`);
copyFileSync(join(bundle, built[0]), ally);
made.push(ally);

if (withHost) {
  const companion = join(source, "target", "release", "carla-companion.exe");
  if (!existsSync(companion)) fail(`${companion} is missing. Run "cargo build --release -p carla-companion" first.`);
  const icon = nsisPath(join(source, "apps", "shell", "src-tauri", "icons", "icon.ico"));

  // 2. The desktop host: the companion, wrapped in an installer of its own.
  const host = join(out, `CarlaOS-Desktop-Host-${version}-setup.exe`);
  nsis("host.nsi", { VERSION: version, COMPANION: nsisPath(companion), ICON: icon, OUTFILE: nsisPath(host) });
  made.push(host);

  // 3. Both together.
  const full = join(out, `CarlaOS-Full-${version}-setup.exe`);
  nsis("full.nsi", { VERSION: version, ALLY_SETUP: nsisPath(ally), HOST_SETUP: nsisPath(host), ICON: icon, OUTFILE: nsisPath(full) });
  made.push(full);
}

const sums = made.map((path) => {
  const name = path.slice(out.length + 1);
  const megabytes = (statSync(path).size / 1_048_576).toFixed(1);
  console.log(`${name}  ${megabytes} MB`);
  return `${createHash("sha256").update(readFileSync(path)).digest("hex")}  ${name}\n`;
});
writeFileSync(join(out, "SHA256SUMS.txt"), sums.join(""));
