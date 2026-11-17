---
name: review-pr
description: Review a CarlaOS pull request for correctness, dependencies, security, privacy, tests, Windows behaviour and actual CI status before merge.
---

# Review a CarlaOS pull request

1. Retrieve the current PR metadata and exact head/base SHAs. Confirm the PR is still open.
2. Inspect the full diff against its stated base. Do not review only the PR description.
3. Inspect relevant callers, state transitions, data migrations, authorization boundaries, tests and documentation.
4. Check whether the change depends on another unmerged PR. If stacked, verify the PR base is its direct parent.
5. Look for secrets, personal data, unsafe process execution, path traversal, privilege errors, untrusted URL handling and unintended outbound data.
6. Check failure paths: offline providers, timeouts, corrupted local data, missing registry keys/files, cancellation, sleep/resume, partial installs and rollback.
7. Review handheld-specific impact: CPU, memory, disk access, network chatter, input/focus, reduced motion and readability.
8. Inspect CI for the exact head SHA. Identify every failed, pending, skipped and passing job. Read logs for failures; distinguish code defects from CI fixture/infrastructure failures.
9. Compare tests claimed in the description with tests actually executed. Do not infer physical Ally validation from CI.
10. Use the `carla-reviewer` agent for an independent read-only pass on high-risk changes. Use the `test-engineer` agent for test selection and CI failure analysis.
11. Return blocking findings first, with file/line, trigger, impact and evidence. Then list non-blocking improvements and unverified risks.
12. Do not merge, close, retarget or force-push the PR. The human owner approves integration.

A PR is ready only when its dependencies are understood, its intended diff is correct, all required checks pass, and the remaining validation limits are explicitly accepted.
