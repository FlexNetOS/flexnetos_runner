---
description: Run the Rust-backed Codex TDD forge-loop seed.
---

Use the repository's Rust forge-loop engine instead of improvising the loop in chat.

Command:

```bash
rtk fxrun forge-loop run --goal "$ARGUMENTS"
```

Rules:
- Follow TDD: prove or create a red test before implementation.
- Do not start another cycle.
- Run self-evaluation on every cycle.
- Mine the required target resources before claiming an upgrade: OpenAI Codex GitHub Action, Permissions, Subagents, RoggeOhta/awesome-codex-cli, and Yeachan-Heo/oh-my-codex.
- Keep auto-compaction enabled for every local, action, and nested `codex exec` session; preserve the active phase, source matrix, validation state, and next action in compact summaries.
- Record source-attributed findings and map each applied change to a config, hook, rule, skill, subagent, permission, GitHub Action, or tool surface.
- Use isolated named worktrees for concurrent loop work; follow `.codex/worktrees/forge-loop-isolation.toml` and do not run multiple mutating loops in the same checkout.
- Spawn project-scoped subagents only for genuinely parallel research/review/CI audit work; wait for all results and synthesize evidence before editing.
- Use research findings to improve reliability, accuracy, and speed.
- If a self-upgrade is warranted, leave the intended repository changes in the working tree; do not run git commit, git push, or gh pr from inside Codex.
- The outer forge-loop engine will commit, push, open a PR with PR title 'chore: forge loop self-upgrade', and auto-merge green PRs when repository settings allow.
- Strict upgrade only: no downgrade/removal unless a replacement is installed, configured, and parity-proven. Complete `.codex/checklists/forge-loop-cycle.toml` evidence before claiming a cycle done.
- Before completion, run `rtk fxrun forge-loop components-audit --strict`, `rtk fxrun forge-loop target-mining-audit --strict`, and `rtk fxrun forge-loop docs-drift --json`; if Codex config surfaces changed, update the component audit and CI guard in the same PR.
