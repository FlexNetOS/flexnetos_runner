# Codex forge-loop deep research exhaustion — 2026-06-27

This report records the target-mining pass used to upgrade the `flexnetos_runner` `.codex` forge-loop. Official OpenAI docs were treated as normative; community repositories were mined for portable patterns only.

## Exhausted targets

| Target | Exhausted material | Applied to forge-loop |
| --- | --- | --- |
| `https://github.com/openai/codex` | Codex Rust CLI behavior, ChatGPT-plan authentication, noninteractive execution, JSON output, sandbox/config flags, and upstream release surface. | `codex_invocation()` pins `codex exec --json --sandbox workspace-write --ignore-user-config` plus explicit continuity config; target-mining audit now verifies the upstream CLI source remains covered. |
| `https://developers.openai.com/codex/config-advanced` | Project config, hooks, rules, custom agents, model flags, sandbox, and auto-compaction controls. | `.codex/config.toml`, compact prompt, hooks/rules/agents, GitHub Action args, and Rust invocation flags encode the config contract. |
| `developers.openai.com/codex/github-action` | Purpose, prerequisites, prompt-file, checkout shape, `codex-args`, model/effort, sandbox, `output-file`, privilege controls, allowlists, structured output, artifact capture, and security checklist. | `.github/workflows/codex-forge-loop.yml` keeps workflow-dispatch prompt/model/effort inputs and output capture while routing scheduled execution through local subscription auth; the schema remains the structured evidence parity target. |
| `developers.openai.com/codex/permissions` | Beta profile status, non-composition with `sandbox_mode`, built-in profiles, workspace roots, filesystem deny precedence, secret/env denial, and network allowlists. | `.codex/permissions/forge-loop-workspace.toml` stays a blueprint while active config still uses `sandbox_mode`; permission hook proves it is blueprint-only. |
| `developers.openai.com/codex/subagents` | Explicit-spawn semantics, built-in agents, project `.codex/agents/*.toml`, required fields, optional nicknames/model/sandbox/MCP, inherited sandbox behavior, max threads/depth, and token/cost cautions. | Narrow forge-loop researcher/auditor/CI-sentinel agents; subagent roster hooks; prompt requires fan-out only for useful parallel work. |
| `RoggeOhta/awesome-codex-cli` | Full README category map: official resources, AGENTS templates, subagents, skills, plugins, hooks, MCP client/server, workflow/session management, model proxies, CI/CD, monitoring/analytics, Docker/sandboxing, cross-agent tools, tutorials, comparisons, shell/terminal, and remote access. | Forge-loop skill and ledgers now require inventory across these categories and reject unguarded/imported ecosystem sprawl. |
| `Yeachan-Heo/oh-my-codex` | Full repository clone plus README/docs: strong-session launch, project/user setup scope, doctor/smoke checks, named worktree safety, deep-interview → plan → durable goal flow, research-before-plan, state/log durability, native hooks, team coordination, troubleshooting/stale-state cleanup. | Adopted only portable patterns: compact prompt, compact hooks with target coverage, explicit worktree guidance, durable evidence schema, and no wholesale external runtime dependency. |
| `https://crates.io` | Rust crates for scheduling, tracing, structured output, evaluation, and reliability were treated as upgrade candidates only when they add guarded value. | No dependency churn in this cycle; the audit now requires crates.io source coverage before future dependency additions. |

## Auto-compaction continuity upgrade

The forge-loop now treats auto-compaction as a required session invariant for local Codex runs, nested `codex exec`, and Codex Action jobs:

- `.codex/config.toml` enables `auto_compaction`, sets `model_auto_compact_token_limit = 3000000`, caps tool output, and points at `.codex/prompts/compact-forge-loop.md`.
- `codex_invocation()` injects the same auto-compaction settings even under `--ignore-user-config`.
- `.github/workflows/codex-forge-loop.yml` captures `codex-forge-loop-output.md` from the subscription-auth engine path.
- `.codex/hooks/forge_loop_compact_summary.py` reports target coverage plus auto-compaction config at `PreCompact` and `PostCompact`.
- `.github/codex/schemas/forge-loop-output.schema.json` requires `auto_compact_continuity` evidence.

## Regression guards

- `fxrun forge-loop components-audit --strict`
- `fxrun forge-loop target-mining-audit --strict`
- `fxrun forge-loop docs-drift --json`
- `cargo test -p runner-cli --all-features forge_loop::tests`
- Full workspace fmt/test/clippy/audit gates
