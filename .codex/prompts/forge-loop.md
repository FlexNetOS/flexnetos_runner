---
description: Run the Rust-backed Codex TDD forge-loop seed.
---

Use the repository's Rust forge-loop engine instead of improvising the loop in chat.

Command:

```bash
fxrun forge-loop run --goal "$ARGUMENTS"
```

Rules:
- Follow TDD: prove or create a red test before implementation.
- Run self-evaluation on every cycle.
- Mine the required target resources before claiming an upgrade: OpenAI Codex GitHub Action, Permissions, Subagents, RoggeOhta/awesome-codex-cli, and Yeachan-Heo/oh-my-codex.
- Keep auto-compaction enabled for every local, action, and nested `codex exec` session; preserve the active phase, source matrix, validation state, and next action in compact summaries.
- Record source-attributed findings and map each applied change to a config, hook, rule, skill, subagent, permission, GitHub Action, or tool surface.
- Use isolated named worktrees for concurrent loop work; do not run multiple mutating loops in the same checkout.
- Spawn project-scoped subagents only for genuinely parallel research/review/CI audit work; wait for all results and synthesize evidence before editing.
- Use research findings to improve reliability, accuracy, and speed.
- Commit, push, open a PR, and auto-merge green PRs when repository settings allow.
- Strict upgrade only: no downgrade/removal unless a replacement is installed, configured, and parity-proven.
- Before completion, run `fxrun forge-loop components-audit --strict`, `fxrun forge-loop target-mining-audit --strict`, and `fxrun forge-loop docs-drift --json`; if Codex config surfaces changed, update the component audit and CI guard in the same PR.
