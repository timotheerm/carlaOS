---
title: CarlaOS project context
type: project
status: active
updated: 2026-11-17
---

# CarlaOS

CarlaOS is a lightweight, pink, console-style gaming shell for the ASUS ROG Xbox Ally (RC73YA) on Windows 11, with a companion application for Windows desktop PCs.

The product goal is to make a handheld feel like a dedicated game console while allowing games to be launched locally, streamed from paired desktops, or opened through supported cloud services. The player-facing experience should stay simple; setup and system administration belong behind an administrator flow.

## Technology

- **Shell:** Tauri 2, React, TypeScript and Vite in `apps/shell`.
- **Core:** Rust shared library and platform logic in `crates/core`.
- **Companion:** Rust desktop companion in `crates/companion`.
- **Data:** SQLite catalogue plus local data and artwork.
- **Streaming:** stock Sunshine on desktop and Moonlight on the Ally, with Tailscale between locations.
- **Build:** Cargo workspace, npm lockfile, GitHub Actions and Windows NSIS installers.

## Product boundaries

- The Ally shell is the primary player interface.
- The desktop companion exposes host information and the services required for streaming and remote administration.
- The shared core owns cross-application models and protocol logic.
- The administrator controls machine changes, pairing and sensitive configuration.
- Windows changes should be reversible and clearly reported.
- Avoid introducing persistent background services or heavy dependencies without a clear product need.

## Repository map

See [[architecture/system-map]] and the detailed source references there.

## Current delivery state

The repository is actively evolving. Do not infer a completed milestone from a plan or an open PR. Read [[operations/current-state]] and check GitHub's open PRs and CI before starting a task.

The main branch is the integration and release branch. Feature work should be done locally on topic branches and reviewed through pull requests. See [[operations/working-agreement]] and [[../docs/GIT_WORKFLOW]].

## Product and implementation references

- [[../docs/ARCHITECTURE]]
- [[../docs/CONSOLE-FEEL]]
- [[../docs/LIBRARY]]
- [[../docs/EMULATORS]]
- [[../docs/CLOUD]]
- [[../docs/POWER]]
- [[../docs/REMOTE-ADMIN]]
- [[../docs/RELEASES]]
