---
title: CarlaOS system map
type: architecture
status: maintained
updated: 2026-11-17
---

# System map

This is a navigation map, not a substitute for reading the code or the approved architecture document.

## Runtime pieces

| Area | Location | Responsibility |
|---|---|---|
| Ally shell UI | `apps/shell/src` | React screens, navigation, design system and frontend state |
| Tauri host | `apps/shell/src-tauri` | Native application lifecycle, commands, Windows integration and shell-side orchestration |
| Shared Rust core | `crates/core` | Catalogue, stores, emulation, launch, power and shared protocol/data models |
| Desktop companion | `crates/companion` | Desktop host status, streaming integration, game discovery and remote administration |
| Product documentation | `docs/` | Detailed behaviour, architecture, features and release procedures |
| Packaging | `packaging/`, `.release-tools/` | Windows installer assembly and smoke tests |
| CI | `.github/workflows/` | Test, build, package and release automation |

## Important boundaries

- Keep shared cross-platform logic in `crates/core` when both shell and companion need it.
- Keep Tauri commands and Windows-only application integration in `apps/shell/src-tauri`.
- Keep player presentation and UI state in `apps/shell/src`; avoid duplicating business rules that already belong in the core.
- The desktop companion is a separate runtime. Do not assume it can access Ally-local files.
- A game can have multiple installs/locations; do not collapse catalogue identity and install location into one concept.
- Administrator-only actions must be enforced by the backend, not merely hidden in the UI.
- Machine-level changes must be reversible and must preserve the previous value needed for rollback.

## Where to begin reading

1. `README.md` — current build and run instructions.
2. `docs/ARCHITECTURE.md` — approved product architecture and boundaries.
3. `docs/GIT_WORKFLOW.md` — branch and integration model.
4. The relevant feature document in `docs/`.
5. The Rust/TypeScript source and its tests.

## Validation map

- Rust workspace: `cargo test --locked` from the repository root.
- Windows-specific Rust and shell checks: `scripts/check-windows.sh` where applicable.
- Frontend type checking and production bundle: `npm run build` from `apps/shell`.
- Windows installer creation and install/uninstall smoke tests: GitHub Actions Build workflow.
- Real Ally behaviour: must be tested on the physical ASUS ROG Xbox Ally. CI and browser previews do not prove controller, power, sleep, display, installer upgrade or handheld behaviour.

## Architecture changes

Material changes to data ownership, application boundaries, protocols, security, persistence or release behaviour require an ADR in `knowledge/decisions/`. Update the relevant existing `docs/` specification as part of the same feature work.
