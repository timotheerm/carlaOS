---
name: review-efficiency
description: Review recent CarlaOS agent usage for unnecessary delegation, excessive Opus use and repeated work, from the agent usage log. On demand only.
argument-hint: "[max log lines, default 300]"
disable-model-invocation: true
---

# Efficiency review

Do not read the raw log or old transcripts. Run only the summariser:

`node .claude/hooks/summarize-agent-usage.mjs $ARGUMENTS` (run it from the repository root; it reads just the last N log lines and prints about ten lines).

Then judge, using the `CLAUDE.md` "Orchestration" rules, and report in ≤ 12 lines:
- **Unnecessary delegation:** many agents per session, bursts of 3 or more, or labels that sound like a small or single-file task the coordinator could do directly.
- **Excess escalation:** any Opus call without a stated reason (security, protocol, hard cross-component, or a failed Sonnet attempt), or `sonnet` used for mechanical work (test runs, git checks, vault edits) that Haiku handles.
- **Repeated work:** the same agent with the same label in one session, or two agents on one question.
- **Data quality:** what is observed vs inferred. A call's model is logged only when the coordinator passed one; otherwise it is the frontmatter fallback, inferred. `SubagentStart` itself carries no model. If the log is empty or the starts and calls disagree, say so instead of guessing.
- **Recommendations:** at most 3 concrete tweaks to `CLAUDE.md` or the agent files. Propose them; do not edit anything unless the user asks.

This is a manual review, not a step in any workflow.
