---
title: "ADR-007: Dynamic agent orchestration"
type: adr
status: accepted
date: 2026-11-17
---

# ADR-007: Dynamic agent orchestration

## Context
Fixed role-to-model mappings and fixed skill pipelines spend tokens on delegation that small tasks do not need.

## Decision
The main session decides per task: direct work or delegation, which specialist(s), which model (`haiku` → `sonnet` → `opus` as escalation only), and whether parallelism is safe. The model is passed per call; agent frontmatter models are fallbacks. The procedure lives only in `CLAUDE.md` ("Orchestration"). The coordinator prints an `Orchestration:` line before delegating and lists agents/models used in its final report.

## Consequences
- Lower cost on routine work; Opus is used only with a stated reason.
- Objective record: hooks in `.claude/settings.json` log agent starts and the coordinator's Agent calls (`.claude/hooks/log-agent.mjs`) to a gitignored JSONL; `/review-efficiency` summarises it. `SubagentStart` does not report the model; the requested model comes from the Agent call, otherwise the agent's frontmatter fallback is inferred.
- Delegation quality depends on the coordinator following `CLAUDE.md`; observability relies on those report lines and the session transcript.
- Approval gates (destructive, device config, security, secrets, release/merge, force-push) are unchanged.
