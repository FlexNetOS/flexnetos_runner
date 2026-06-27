---
name: forge-loop-research
description: Research Codex forge-loop improvements across Codex CLI, crates.io, kclaw0, awesome-codex-cli, and oh-my-codex; use when improving the Rust fxrun forge-loop reliability, accuracy, or speed.
---

# Forge Loop Research

When invoked, research improvements for the Rust `fxrun forge-loop` engine.

Required sources:
- `https://github.com/openai/codex` for current Codex CLI behavior and issues.
- `https://developers.openai.com/codex/config-advanced` for project config, hooks, rules, custom agents/subagents, profiles, model flags, sandbox, and approval settings.
- `https://developers.openai.com/codex/github-action` for `openai/codex-action` prompt-file, `codex-args`, model/effort, sandbox, output-file, and safety controls.
- `https://github.com/RoggeOhta/awesome-codex-cli` for ecosystem tools, skills, plugins, MCP servers, and orchestration patterns.
- `https://github.com/Yeachan-Heo/oh-my-codex` for multi-agent/team orchestration patterns.
- crates.io for Rust crates that improve scheduling, tracing, structured output, evaluation, and reliability.
- `https://github.com/drdave-flexnetos/kclaw0` plus local `docs/kclaw0-upgrade-ledger.md` for dark-factory/self-upgrade governance.

Output format:
- one-line summary
- source-attributed findings
- loop component/config inventory (`config`, hooks, rules, skills, custom agents/subagents, model flags, GitHub Action/tool surfaces)
- one recommended smallest safe self-upgrade
- tests required before merge

Constraints:
- Prefer strict-upgrade-only changes.
- Do not recommend downgrades or removals without parity proof.
- Prefer improvements that can be evaluated automatically on every loop run.
- When changing Codex loop components or configs, update `fxrun forge-loop components-audit` and CI so the new surface is machine-checkable.
