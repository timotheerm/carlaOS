---
title: ADR-002 Provider-neutral project memory
type: decision
status: accepted
date: 2026-11-17
---

# ADR-002: Keep project memory provider-neutral

## Context

CarlaOS is being developed with Claude Code and may also be reviewed or analysed by Astra/ChatGPT and local models. Memory stored only in one provider's chat history or proprietary project feature would make context difficult to share, version and verify.

## Decision

Keep durable project memory as Markdown in the repository under `knowledge/`. Keep Claude-specific instructions, agents and skills under `.claude/` and `CLAUDE.md`. Keep product specifications in `docs/` and implementation truth in the code/tests. Obsidian is an optional interface to the Markdown repository, not the database or source of truth.

## Alternatives considered

- Claude-only memory or provider-specific project storage: rejected because other models and local tools could not reliably share it.
- A separate personal vault as the only source: rejected because it would drift from code and PR history.
- A vector database or embedding service immediately: deferred because the current knowledge volume is small and Markdown search is sufficient.

## Consequences

- All models can read the same notes.
- Notes can be reviewed, diffed and reverted through Git.
- A human can edit them in Obsidian or VS Code.
- Memory maintenance must distinguish verified facts from proposals and must not duplicate detailed feature documents.
- Semantic retrieval, SQLite or embeddings can be introduced later only if plain Markdown and ripgrep become inadequate.

## References

- [[../START-HERE]]
- [[../operations/second-model-review]]
