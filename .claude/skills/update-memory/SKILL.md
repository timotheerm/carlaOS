---
name: update-memory
description: Record a CarlaOS decision, status change, found/fixed issue or session summary in the knowledge/ vault. Use only when one of those happened.
argument-hint: "[what changed]"
---

# Update memory: $ARGUMENTS

Scope: `knowledge/` only. Touch the fewest notes.

1. Evidence: what was actually done this session (`git log --oneline -5`, `git diff --stat` if needed, test output you have). Do not re-derive what you already know.
2. Map change → note (read only the notes you will edit):
   - feature status/verification → `development/current-state.md` (that area's row)
   - issue found/fixed → `development/known-issues.md` (next free `KI-NNN`; fixed → Resolved)
   - lasting decision → new `decisions/ADR-NNN-title.md` (`templates/decision-record.md`) + row in `decisions/README.md`
   - scope/priority → `development/roadmap.md`
   - session summary (only if meaningful progress) → top of `development/session-log.md`, ≤ 6 lines (`templates/session-entry.md`)
3. Levels: "in source" only if you saw the code; "tested" only if tests exist and say whether they ran; "hardware-verified" only on the user's report (record date and device). No secrets or personal data.
4. Edit minimally, keep relative links valid, bump `updated:`.
5. One or two notes → do it yourself. Three or more notes or an audit → one `memory-curator` call with the evidence above.
6. Report in ≤ 5 lines: notes changed, claims added/removed, open questions. Do not commit.
