# Forge-loop compaction summary contract

When auto-compacting a forge-loop session, preserve only durable execution state that lets the next turn continue without context rot:

1. Active goal, branch, PR, cycle manifest path, and active phase.
2. The current TDD phase: red, implement, gate, evaluate, research, or upgrade.
3. Source-mining coverage for every required target:
   - Codex GitHub Action
   - Codex permissions
   - Codex subagents
   - RoggeOhta/awesome-codex-cli
   - Yeachan-Heo/oh-my-codex
4. Applied surfaces changed so far: `.codex/config.toml`, `.codex/hooks.json`, `.codex/prompts/*`, `.codex/agents/*`, `.codex/permissions/*`, `.agents/skills/*`, `.github/workflows/*`, `.github/codex/*`, Rust verifier code, and docs.
5. Validation already run and exact remaining validation.
6. Dirty files that are user-owned and must not be reverted.
7. Next single action / next action.

Do not preserve long command logs, full source excerpts, or repeated repository status. Keep the summary source-attributed and checkpoint-oriented.
