# Codex target mining ledger

Updated: 2026-06-27

This ledger records the current additive extraction from the required deep-research targets into the flexnetos_runner `.codex` forge-loop harness.

| Target | Extracted upgrade pressure | Applied harness surface |
| --- | --- | --- |
| `developers.openai.com/codex/github-action` | Use `prompt-file`, JSON-array `codex-args`, model/effort, sandbox, `output-file`, privilege controls, artifact capture, and structured output via `--output-schema`. | `.github/workflows/codex-forge-loop.yml`; `.github/codex/schemas/forge-loop-output.schema.json`; components-audit guard. |
| `developers.openai.com/codex/permissions` | Permission profiles are beta and must not be mixed with active `sandbox_mode`; use them as least-privilege migration contracts with deny rules for secrets and scoped network allowlists. | `.codex/permissions/forge-loop-workspace.toml` is an audited blueprint, not an active config layer while `.codex/config.toml` still uses `sandbox_mode`. |
| `developers.openai.com/codex/subagents` | Define narrow project-scoped custom agents; cap fan-out with `[agents]`; keep recursive depth shallow; use read-only agents for research/review. | Added `forge-loop-researcher` and `forge-loop-ci-sentinel`; retained `forge-loop-auditor`; added SubagentStart/SubagentStop roster hook. |
| `RoggeOhta/awesome-codex-cli` | The ecosystem emphasizes composable hooks, MCP/memory/cost tracking, workflow management, CI automation, and specialized subagents/skills. | Added source-mining researcher role, CI sentinel, permission blueprint, structured action output, and component inventory checks for these surfaces. |
| `Yeachan-Heo/oh-my-codex` | OMX patterns favor named isolated worktrees, durable plan/log state, doctor checks, native hook mapping, teams only when useful, and HUD/runtime status. | Forge-loop prompt now requires isolated worktrees, target-mining evidence, component inventory, and subagent/team fan-out only for parallelizable work. |
| `drdave-flexnetos/kclaw0` | The kclaw0 Dark Factory target requires 24/7 autonomous operation, 300-agent / 4000-step / 12+ hour persistence evidence, heartbeat scheduling, self-upgrade pipeline discipline, immutable governance, holdout validation, per-node budget caps, and a GitHub label state machine. | Runner Sustain and Runner Black Factor Watch keep self-hosted lanes doing useful auditable work; `runner-black-factor-audit --strict`, `runner-ops-slo-audit --strict`, and `runner-fleet-audit --strict` convert the kclaw0 persistence, unattended-operations, and shared-fleet ownership requirements into machine gates. |

## 2026-06-27 auto-compaction continuity extraction

All six target families now feed the `.codex` continuity contract: local config enables `auto_compaction`, `model_auto_compact_token_limit`, scoped tool-output limits, and `experimental_compact_prompt_file`; `codex exec` invocations and the GitHub Action pass the same settings explicitly; `PreCompact`/`PostCompact` hooks emit target coverage; and the structured output schema requires `auto_compact_continuity` evidence. This applies the OpenAI context-window/compaction guidance, the OMX durable-state pattern, and the kclaw0 24/7 dark-factory requirement without adopting an external runtime.
