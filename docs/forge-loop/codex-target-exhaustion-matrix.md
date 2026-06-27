# Codex forge-loop target exhaustion matrix

Updated: 2026-06-27

This matrix is the current "mined to the bone" checklist for the explicit target set. It separates source extraction from the local `.codex` application and the verifier that prevents regressions.

| Target | Exhausted extraction categories | Applied local surfaces | Regression guard |
| --- | --- | --- | --- |
| `developers.openai.com/codex/github-action` | Trigger authorization, prompt source, `codex-args`, model/effort, sandbox, privilege strategy, final-message output, artifact output, structured schema, troubleshooting evidence. | `.github/workflows/codex-forge-loop.yml`; `.github/codex/prompts/forge-loop.md`; `.github/codex/schemas/forge-loop-output.schema.json`. | `codex_github_action_workflow_uses_documented_controls`; `target-mining-audit`. |
| `developers.openai.com/codex/permissions` | Built-in profiles, custom `default_permissions`, filesystem precedence, secret deny globs, bounded glob scan, network domain allowlists, local/private-network caution, no mixing with `sandbox_mode`. | `.codex/permissions/forge-loop-workspace.toml`; `.codex/hooks/forge_loop_permission_request.py`; active `.codex/config.toml` intentionally keeps `sandbox_mode` and omits `default_permissions`. | `codex_deep_target_mining_surfaces_are_guarded`; `target-mining-audit`. |
| `developers.openai.com/codex/subagents` | Built-ins, project `.codex/agents/*.toml`, required fields, optional model/sandbox/skills/MCP config, `nickname_candidates` display labels, fan-out caps, inherited sandbox behavior, explicit spawn requirement. | `.codex/agents/forge-loop-auditor.toml`; `.codex/agents/forge-loop-researcher.toml`; `.codex/agents/forge-loop-ci-sentinel.toml`; SubagentStart/SubagentStop hooks. | `components-audit --strict`; `target-mining-audit`. |
| `RoggeOhta/awesome-codex-cli` | Ecosystem categories: AGENTS templates, subagents, skills, plugins, hooks, MCP, workflow/session managers, CI/CD, Monitoring/analytics, Docker/sandboxing, cross-agent tools. | Forge-loop research skill requires inventory across those categories; target ledger records extracted pressure; components-audit includes tools, skills, agents, hooks, docs. | `forge_loop_skill_references_codex_config_and_action_docs`; `target-mining-audit`. |
| `Yeachan-Heo/oh-my-codex` | Strong-session launch, named worktree isolation, doctor/smoke tests, deep-interview -> plan -> durable-goal path, native hook mapping, durable state/logs, teams only for parallelizable work, stale-team cleanup. | Forge-loop prompt requires source-attributed mining, isolated named worktrees, explicit subagent fan-out only when useful, and final component/docs-drift gates; compact hooks preserve target coverage across context churn. | prompt mirror tests plus `target-mining-audit`. |

## Remaining policy

A future loop may add more surfaces, but it must not claim a target has been mined unless the extraction category, local application, and regression guard are all updated together. If official Codex docs change, update this matrix and `fxrun forge-loop target-mining-audit --strict` in the same PR.
