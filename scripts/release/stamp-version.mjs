#!/usr/bin/env node
// Write a version number into every place a build reads it from, so the
// installers, the programs and the Settings page all say the same thing.
//
//   node stamp-version.mjs 0.5.0 [repository root]
//
// The numbers in the repository are placeholders: releases are numbered by
// their git tag (see docs/RELEASES.md) and stamped here at build time.
import { readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const [version, rootArg = "."] = process.argv.slice(2);
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error("Give a version such as 0.5.0 (three numbers, nothing else).");
  process.exit(1);
}
const root = resolve(rootArg);

function edit(path, change) {
  const file = join(root, path);
  const before = readFileSync(file, "utf8");
  const after = change(before);
  if (after === null) {
    console.error(`${path}: no version found to replace.`);
    process.exit(1);
  }
  writeFileSync(file, after);
  console.log(`${path}: ${version}`);
}

// Cargo.toml: the one version every crate inherits.
edit("Cargo.toml", (text) => {
  const pattern = /(\[workspace\.package\][^\[]*?\nversion\s*=\s*")[^"]*(")/;
  return pattern.test(text) ? text.replace(pattern, `$1${version}$2`) : null;
});

// Cargo.lock: our own crates are the entries without a `source`. They have to
// match Cargo.toml or `cargo build --locked` refuses to build.
edit("Cargo.lock", (text) => {
  let found = 0;
  const entries = text.split(/(?=^\[\[package\]\]$)/m).map((entry) => {
    if (!entry.startsWith("[[package]]") || /^source\s*=/m.test(entry)) return entry;
    found += 1;
    return entry.replace(/^version\s*=\s*"[^"]*"/m, `version = "${version}"`);
  });
  return found ? entries.join("") : null;
});

// The shell's installer and window take their version from Tauri's file.
edit("apps/shell/src-tauri/tauri.conf.json", (text) => {
  const pattern = /^(\s*"version"\s*:\s*")[^"]*(")/m;
  return pattern.test(text) ? text.replace(pattern, `$1${version}$2`) : null;
});
