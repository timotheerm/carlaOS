---
title: "ADR-005: Stock Sunshine + Moonlight over Tailscale"
type: adr
status: accepted
date: 2026-11-11
note: Retroactive record of a decision in docs/ARCHITECTURE.md
---

# ADR-005: Stock Sunshine + Moonlight over Tailscale

## Context
Streaming from desktops to the Ally must work across locations with zero player setup.

## Decision
Use unmodified Sunshine (desktop) and Moonlight (Ally), started hidden through its command line.
Tailscale carries traffic between locations. No router ports are opened. The companion manages Sunshine.

## Consequences
- No custom streaming protocol to maintain; behaviour depends on upstream changes.
- Moonlight window behaviour and pairing flows are a recurring risk ([KI-001](../development/known-issues.md)).
- Source: [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md), [docs/STREAMING.md](../../docs/STREAMING.md).
