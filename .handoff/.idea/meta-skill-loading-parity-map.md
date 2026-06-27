# Meta Skill Loading Parity Map

Created: 2026-06-26
Purpose: first v4 deliverable for the agentic PR failure repair queue research plan.

## Executive summary

Claude and Codex both have access to the `meta` concepts, but they currently reach them through different loading paths and with different reliability:

- **Claude path:** repo-local `.claude/` files plus the `meta` Claude plugin/marketplace model. `meta init claude` is first-class in `meta_cli` and installs skills/rules/hooks directly into `.claude/`.
- **Codex path:** project `.codex/` config/prompts/hooks plus globally installed Codex plugin cache entries under `~/.codex/plugins/cache/gitkb/meta/...`. Codex sees the `meta` plugin as skills, but not as Claude-style slash commands. Codex also has project MCP config for `meta-mcp`, but in this session the practical reliable path is still shelling to `meta`/`rtk meta` and using loaded skill text/tool_search context.
- **Shared source of truth:** the actual operational substrate is the `meta` CLI + `.meta.yaml` + `meta-mcp` + `meta` plugin package. The text surfaced to Claude and Codex is copied/generated from more than one place and is currently drifting.

The parity path is therefore not to invent new agent instructions. The parity path is to make the `meta` plugin package and `meta_cli` init/sync machinery produce equivalent Claude and Codex surfaces from one canonical source, with a mandatory startup contract: `meta project list --json`, `meta git status`, and `meta context`/MCP availability.

## Evidence inspected

Local roots inspected:

```text
/home/drdave/Desktop/meta/.claude
/home/drdave/Desktop/meta/.codex
/home/drdave/Desktop/meta/claude-plugin
/home/drdave/Desktop/meta/claude-plugins
/home/drdave/Desktop/meta/codex
/home/drdave/Desktop/meta/meta-plugins
/home/drdave/Desktop/meta/meta_cli
/home/drdave/Desktop/meta/meta_mcp
/home/drdave/Desktop/meta/meta_plugin_api
/home/drdave/Desktop/meta/meta_plugin_protocol
/home/drdave/.codex/plugins/cache/gitkb/meta/0.1.0
/home/drdave/.codex/config.toml
```

Key commands/evidence:

```bash
meta project list --json
meta plugin list
meta --help
rg -n 'claude|codex|skills|plugin|mcp' meta_cli/src meta_mcp/src ...
sha256sum claude-plugin/skills/*/SKILL.md ~/.codex/plugins/cache/gitkb/meta/0.1.0/skills/*/SKILL.md .claude/skills/*.md
```

## Claude loading path

### 1. Repo-local `.claude/` surface

The meta root has a fully populated Claude surface:

```text
.claude/settings.json
.claude/commands/*.md
.claude/skills/meta-workspace.md
.claude/skills/meta-git.md
.claude/skills/meta-exec.md
.claude/skills/meta-plugins.md
.claude/skills/meta-worktree.md
.claude/skills/meta-safety.md
.claude/rules/*.md
.claude/hooks/*.sh
.claude/agents/*.md
```

Important local settings:

- `SessionStart` runs `meta context`.
- `SessionStart` starts `git kb serve` if needed.
- `PreCompact` runs `meta context` and handoff checkpoint hooks.
- `PreToolUse` runs a destructive-command guard.
- `.claude/commands/resume.md` explicitly reconstructs context from handoff, fleet, kb, and live `meta git status`.
- `.claude/commands/fleet.md` uses the handoff fleet board.

This gives Claude direct, repo-local operational affordances: slash commands, skills, rules, hooks, and agents.

### 2. `meta init claude` is first-class in `meta_cli`

`meta_cli/src/init.rs` is explicitly Claude-focused:

- It defines `InitCommand::Claude`.
- It embeds skill files with `include_str!("../.claude/skills/...")`.
- It embeds rules with `include_str!("../.claude/rules/...")`.
- It installs into `.claude/skills` and `.claude/rules`.
- It merges `.claude/settings.json` hook config.
- It attempts to register a Claude plugin marketplace with:

```bash
claude plugin marketplace add FlexNetOS/claude-plugins
```

Then it tells the user:

```bash
claude plugin install meta@gitkb
```

This is the strongest evidence that Claude is the historical primary target for `meta` skill distribution.

### 3. Claude plugin package

The plugin source exists at:

```text
/home/drdave/Desktop/meta/claude-plugin
```

Important files:

```text
claude-plugin/.claude-plugin/plugin.json
claude-plugin/.mcp.json
claude-plugin/hooks/hooks.json
claude-plugin/skills/meta-*/SKILL.md
```

The plugin manifest names the plugin `meta` and describes it as:

```text
Multi-repo workspace management. Skills, hooks, and MCP server for the meta CLI.
```

The plugin MCP config registers:

```json
{
  "mcpServers": {
    "meta": {
      "command": "meta-mcp",
      "args": []
    }
  }
}
```

### 4. Claude marketplace

The local `claude-plugins` marketplace includes:

```text
plugin: meta
source: git-subdir
url: gitkb/meta
path: claude-plugin
version: 0.1.0
```

So Claude has two paths:

1. repo-local `.claude/` installation via `meta init claude`, and
2. plugin marketplace install via `meta@gitkb`.

## Codex loading path

### 1. Project `.codex/` surface

The meta root has a project-scoped Codex config:

```text
.codex/config.toml
.codex/hooks.json
.codex/hooks/meta-context-session-start.sh
.codex/prompts/meta-status.md
.codex/prompts/meta-worker.md
.codex/prompts/meta-upgrade.md
.codex/prompts/codex-rust-forge.md
.codex/agents/meta-worker.toml
.codex/rules/strict-upgrade.md
```

Important behavior:

- `project_root_markers = [".git", ".meta.yaml"]`
- hooks are enabled
- child agents are enabled
- the meta MCP server is configured:

```toml
[mcp_servers.meta]
command = "meta-mcp"
args = []
enabled = true
required = false
startup_timeout_sec = 10
tool_timeout_sec = 60
```

- git-kb MCP is configured:

```toml
[mcp_servers.gitkb]
command = "git"
args = ["kb", "mcp"]
```

- `SessionStart` in `.codex/hooks.json` runs:

```bash
rtk bash "$HOME/Desktop/meta/.codex/hooks/meta-context-session-start.sh"
```

That script runs:

```bash
rtk meta context
```

and returns it as additional session context.

### 2. Codex prompts are not equivalent to Claude slash commands

Codex has prompt files such as:

```text
.codex/prompts/meta-status.md
.codex/prompts/meta-worker.md
```

They are useful, but this is not equivalent to the richer Claude `.claude/commands` set.

Examples:

- Claude has `/resume`, `/fleet`, `/handoff`, `kb-*`, etc.
- Codex currently has fewer prompt front doors and relies more on project hooks, developer instructions, and manually loaded/plugin skills.

### 3. Codex global plugin install/cache

The global Codex config has:

```toml
[marketplaces.gitkb]
source_type = "local"
source = "/home/drdave/Desktop/meta/claude-plugins"

[plugins."meta@gitkb"]
enabled = true

[skills]
include_instructions = true
```

The installed Codex plugin cache contains:

```text
/home/drdave/.codex/plugins/cache/gitkb/meta/0.1.0/.claude-plugin/plugin.json
/home/drdave/.codex/plugins/cache/gitkb/meta/0.1.0/.mcp.json
/home/drdave/.codex/plugins/cache/gitkb/meta/0.1.0/hooks/hooks.json
/home/drdave/.codex/plugins/cache/gitkb/meta/0.1.0/skills/meta-exec/SKILL.md
/home/drdave/.codex/plugins/cache/gitkb/meta/0.1.0/skills/meta-git/SKILL.md
/home/drdave/.codex/plugins/cache/gitkb/meta/0.1.0/skills/meta-plugins/SKILL.md
/home/drdave/.codex/plugins/cache/gitkb/meta/0.1.0/skills/meta-safety/SKILL.md
/home/drdave/.codex/plugins/cache/gitkb/meta/0.1.0/skills/meta-workspace/SKILL.md
/home/drdave/.codex/plugins/cache/gitkb/meta/0.1.0/skills/meta-worktree/SKILL.md
```

In the Codex environment, these surface as plugin skills such as:

```text
meta:meta-exec
meta:meta-git
meta:meta-plugins
meta:meta-safety
meta:meta-workspace
meta:meta-worktree
```

They are loaded/used via Codex skill discovery behavior, not through Claude-style slash commands.

### 4. Codex MCP gap in practice

Although `.codex/config.toml` configures `meta-mcp`, the available tool surface in this session did not expose direct `meta_*` MCP tool namespaces. The reliable path available to Codex here is:

- shelling to `meta`/`rtk meta`,
- reading local skill files,
- using `tool_search` to reveal plugin-contributed skill metadata,
- relying on `.codex` session hooks for `meta context` injection.

This means Codex has conceptual skill access but not consistently the same direct command/MCP affordance as Claude.

## Shared source of truth

The operational source of truth is not `.claude` or `.codex` by themselves. It is:

```text
.meta.yaml                         # fleet graph: repos, tags, provides, depends_on
meta_cli                           # CLI command dispatcher and init/sync support
meta_core                          # shared meta primitives
meta_mcp                           # MCP tool server for agents
meta_plugin_protocol               # subprocess plugin JSON protocol
meta_plugin_api                    # older/dynamic plugin trait API
.meta/plugins/meta-git             # project-local plugin executable
.meta/plugins/meta-project         # project-local plugin executable
.meta/plugins/meta-rust            # project-local plugin executable
~/.meta/plugins                    # global installed plugins, if any
PATH meta-* executables            # bundled/system plugins
claude-plugin                      # current plugin package shape
claude-plugins                     # local marketplace consumed by Claude/Codex config
```

The current `meta` command availability confirms project-local plugins are active:

```text
meta plugin list
  dashboard  /home/drdave/.local/bin/meta-dashboard
  git        /home/drdave/Desktop/meta/.meta/plugins/meta-git
  project    /home/drdave/Desktop/meta/.meta/plugins/meta-project
  rust       /home/drdave/Desktop/meta/.meta/plugins/meta-rust
```

`meta --help` exposes:

```text
context
exec
init
plugin
sync
git        [plugin]
project    [plugin]
rust       [plugin]
dashboard  [plugin]
```

## Meta command/plugin dispatch path

`meta_cli/src/subprocess_plugins.rs` shows the actual plugin discovery order:

1. `.meta/plugins/` directories walking upward from cwd,
2. `~/.meta/plugins/`,
3. `PATH` entries named `meta-*`.

A plugin is loaded by executing:

```bash
meta-foo --meta-plugin-info
```

and executing through:

```bash
meta-foo --meta-plugin-exec
```

with a JSON `PluginRequest` sent on stdin.

`meta_plugin_protocol/src/lib.rs` defines the protocol:

- `PluginInfo`
- `PluginRequest`
- `PluginRequestOptions`
- `ExecutionPlan`
- `PlannedCommand`
- `CommandResult`

Plugins return execution plans and `meta_cli` executes them through `loop_lib`.

This matters for PR repair because a future `meta pr-repair` can be either:

- a subprocess plugin that returns plans,
- a built-in command,
- or a higher-level supervisor that shells to existing `meta` commands.

## Meta MCP tool path

`meta_mcp/src/main.rs` exposes a large tool list. Important tool names found in source:

```text
meta_list_projects
meta_exec
meta_get_config
meta_get_project_path
meta_git_status
meta_git_pull
meta_git_push
meta_git_fetch
meta_git_diff
meta_git_branch
meta_git_add
meta_git_commit
meta_git_checkout
meta_git_multi_commit
meta_detect_build_systems
meta_run_tests
meta_build
meta_clean
meta_search_code
meta_get_file_tree
meta_list_plugins
meta_query_repos
meta_workspace_state
meta_analyze_impact
meta_execution_order
meta_snapshot_create
meta_snapshot_list
meta_snapshot_restore
meta_batch_execute
```

This is very close to the PR repair supervisor's needs. The parity issue is making sure Codex can actually call this tool set as reliably as Claude/plugin consumers can.

## Gaps

### Gap 1 — Claude has first-class `meta init`; Codex does not

`meta_cli/src/init.rs` supports `meta init claude` and `meta init ollama`, but no equivalent `meta init codex` was found.

Current Codex support exists as local project files under `.codex/`, but it is not installed/generated by the same typed `meta_cli` init path.

Impact:

- Claude parity is reproducible by command.
- Codex parity currently depends on bespoke project config and global plugin cache state.

Recommended fix:

- Add or design `meta init codex` / `meta sync codex-skills` equivalent in future work.

### Gap 2 — Skill text is copied in multiple places and drifting

Checksums differ among:

```text
claude-plugin/skills/meta-*/SKILL.md
~/.codex/plugins/cache/gitkb/meta/0.1.0/skills/meta-*/SKILL.md
.claude/skills/meta-*.md
meta_cli/.claude/skills/meta-*.md
```

Observed example:

- `claude-plugin/skills/meta-workspace/SKILL.md` uses `rtk meta ...` in examples.
- Codex plugin cache `meta-workspace/SKILL.md` uses `meta ...`.
- root `.claude/skills/meta-workspace.md` lacks plugin frontmatter and also uses `meta ...`.

Impact:

- Claude and Codex can be taught subtly different operational habits.
- PR repair agents may diverge on whether to use `meta` or `rtk meta`.
- Updates to one surface may not reach the others.

Recommended fix:

- Define canonical skill source and generation targets.
- Prefer `rtk meta` in Codex/project hooks where path resolution matters, while documenting plain `meta` as acceptable when resolved correctly.
- Add parity check that diffs/canonicalizes core guidance across surfaces.

### Gap 3 — Claude slash commands are richer than Codex prompt front doors

Claude has repo-local commands:

```text
.claude/commands/resume.md
.claude/commands/fleet.md
.claude/commands/handoff.md
.claude/commands/kb-*.md
```

Codex has only a smaller prompt set:

```text
.codex/prompts/meta-status.md
.codex/prompts/meta-worker.md
.codex/prompts/meta-upgrade.md
.codex/prompts/codex-rust-forge.md
```

Impact:

- Claude has better durable-state recovery affordances.
- Codex depends more on user/developer context and hooks.

Recommended fix:

- Mirror the important Claude commands as Codex prompts or skills:
  - resume
  - fleet
  - handoff
  - kb-context/status/tasks/board
  - pr-repair scan/classify/lane/watch once designed

### Gap 4 — MCP configured for Codex but not guaranteed surfaced

Project Codex config declares `mcp_servers.meta`, but direct `meta_*` MCP tools were not exposed in the active tool namespace during this session.

Impact:

- Codex cannot assume `meta_workspace_state` etc. are callable.
- Agent prompts must include command-line fallback.

Recommended fix:

- Make all Codex meta workflows specify both paths:
  1. preferred MCP tool when exposed,
  2. shell fallback via `rtk meta ...`.
- Investigate Codex MCP loading/logging separately if direct tool access is required.

### Gap 5 — Plugin package identity points at `gitkb/meta` in cache

The Codex cache plugin manifest has homepage/repository as `https://github.com/gitkb/meta`, while local current source is `FlexNetOS/meta` and the local marketplace entry references `gitkb/meta` as a git-subdir source.

Impact:

- Source-of-truth ownership is ambiguous: `gitkb/meta` vs `FlexNetOS/meta`.
- Future updates may come from the wrong remote or stale marketplace source.

Recommended fix:

- Decide whether `FlexNetOS/meta` is canonical for this environment.
- If yes, update marketplace/plugin metadata in the source path later.
- Do not do this as part of PR repair until parity source is decided.

## Recommended parity path

### Phase 0 — Working rule now

Until parity is fixed, all Codex PR-repair instructions should explicitly say:

```text
Start from /home/drdave/Desktop/meta.
Run `rtk meta project list --json` and `rtk meta git status`.
Use `rtk meta` / `meta` command-line operations as the reliable path.
Use meta MCP tools only if exposed in the session.
Use meta plugin skills as conceptual guidance, not as proof of command availability.
```

### Phase 1 — Canonicalize source surfaces

Pick a canonical source for the six core meta skills:

```text
meta-workspace
meta-git
meta-exec
meta-plugins
meta-worktree
meta-safety
```

Recommended canonical source:

```text
/home/drdave/Desktop/meta/claude-plugin/skills/*/SKILL.md
```

Reason: this has plugin frontmatter and is already used by the plugin package model consumed by Codex cache.

But before adopting it, decide the `rtk meta` vs `meta` command style:

- For local project hooks and Codex prompts, `rtk meta` is safer because it routes through the meta-hosted tool resolution layer.
- For generic plugin docs, `meta` is more portable.

Possible compromise:

```text
Use `rtk meta ...` inside this FlexNetOS meta workspace when available; use `meta ...` as the portable command name elsewhere.
```

### Phase 2 — Add Codex init/sync equivalent

Design later implementation:

```bash
meta init codex
meta sync codex-skills
```

Expected outputs:

```text
.codex/config.toml
.codex/hooks.json
.codex/prompts/meta-status.md
.codex/prompts/meta-resume.md
.codex/prompts/meta-fleet.md
.codex/prompts/meta-pr-repair.md
.codex/agents/meta-worker.toml
.codex/rules/meta-workspace-discipline.md
```

This should be generated from the same registry/source as Claude skills.

### Phase 3 — MCP/tool parity validation

Create a small parity verifier:

```bash
meta parity check agent-surfaces
```

or script first:

```bash
scripts/check-agent-surface-parity.sh
```

Checks:

- `.claude/skills` present and current.
- `.codex/prompts` present and current.
- `meta@gitkb` plugin installed/enabled in `~/.codex/config.toml`.
- `mcp_servers.meta` configured in project `.codex/config.toml`.
- `meta-mcp` executable available.
- `meta_mcp` exposes required tools.
- command fallback `rtk meta project list --json` works.

### Phase 4 — PR repair prompt/command parity

Once the PR repair supervisor is designed, add matching surfaces:

Claude:

```text
.claude/commands/pr-repair.md
.claude/skills/pr-repair/SKILL.md or plugin skill
```

Codex:

```text
.codex/prompts/pr-repair.md
.codex/agents/pr-repair-worker.toml
plugin skill if packaged
```

Both should use the same underlying command/substrate:

```bash
meta pr-repair scan
meta pr-repair lane create FlexNetOS/meta#66
meta pr-repair watch
```

## Implications for PR repair supervisor

### 1. Supervisor must not depend on chat-surface-specific features

The supervisor should be CLI/MCP-backed, not Claude-only or Codex-only.

Use:

```text
meta CLI
meta MCP
GitHub CLI/API
worktree metadata
handoff/ICM/Beads after state decision
```

Avoid:

```text
Claude-only slash command semantics as core state
Codex-only prompt files as core state
```

### 2. Repair agent instructions must be surface-neutral

Every repair ticket should include concrete commands and paths, not assume a specific agent knows `/meta:meta-worktree`.

Example:

```text
Reliable start:
cd /home/drdave/Desktop/meta
rtk meta project list --json
rtk meta git status
rtk meta worktree status <lane>
```

### 3. Codex needs explicit meta context injection until parity is solved

For Codex repair workers, ticket prompts should include:

- project key,
- repo path,
- worktree path,
- exact meta commands,
- CI log links,
- validation commands,
- publish/PR rule.

Do not rely on the worker having the same Claude command library.

### 4. MCP availability should be opportunistic, not required

If `meta_workspace_state` is available, use it. If not, use:

```bash
rtk meta project list --json
rtk meta git status --json
```

The PR repair design should require command-line parity first; MCP parity can improve speed and structure.

### 5. Add parity research before implementation

Before implementing `meta pr-repair`, complete these follow-up research files:

```text
/home/drdave/.handoff/.idea/pr-repair-control-plane-map.md
/home/drdave/.handoff/.idea/pr-repair-state-decision.md
/home/drdave/.handoff/.idea/meta-pr-repair-command-sketch.md
```

## Parity checklist

```yaml
meta_skill_parity:
  fleet_graph:
    claude: meta context hook + meta skills
    codex: rtk meta context hook + config root markers
    gap: codex must explicitly run project/status in prompts
    parity_path: keep rtk meta startup contract in all Codex agents

  slash_or_prompt_frontdoors:
    claude: .claude/commands/resume.md, fleet.md, handoff.md, kb-*.md
    codex: .codex/prompts/meta-status.md, meta-worker.md, meta-upgrade.md
    gap: Codex prompt set is smaller
    parity_path: add Codex prompts for resume/fleet/handoff/kb/pr-repair

  core_meta_skills:
    claude: .claude/skills and claude-plugin/skills
    codex: ~/.codex/plugins/cache/gitkb/meta/0.1.0/skills surfaced as plugin skills
    gap: files drift; loading style differs
    parity_path: canonicalize source and add parity verifier

  hooks:
    claude: .claude/settings.json SessionStart/PreCompact/PreToolUse/Stop
    codex: .codex/hooks.json SessionStart/PreCompact/PreToolUse/Stop/SubagentStop
    gap: roughly similar but independently maintained
    parity_path: generate or verify both from a shared policy

  mcp:
    claude: claude-plugin/.mcp.json registers meta-mcp
    codex: .codex/config.toml registers meta-mcp and ~/.codex plugin cache has .mcp.json
    gap: direct meta_* tools not guaranteed exposed in active Codex session
    parity_path: verify MCP exposure; always provide CLI fallback

  plugin_source:
    claude: claude-plugins marketplace references gitkb/meta:claude-plugin
    codex: ~/.codex config marketplace gitkb points to local claude-plugins; plugin cache has gitkb metadata
    gap: FlexNetOS vs gitkb canonical source ambiguity
    parity_path: decide and align plugin metadata later
```

## Recommended next action

Next deliverable should be:

```text
/home/drdave/.handoff/.idea/pr-repair-control-plane-map.md
```

It should map:

- supervisor responsibilities,
- `meta worktree` lane lifecycle,
- `meta exec` validation,
- `meta git` publish/status,
- handoff/ICM/.kb/grit/Beads roles,
- and the exact surface where `meta pr-repair` should live.

## Bottom line

The gap is not that Codex lacks all meta access. Codex has much of it through project hooks, MCP config, and the installed `meta@gitkb` plugin cache. The gap is that Claude has a first-class, generated `.claude`/plugin/command workflow, while Codex has a partly hand-built `.codex` surface plus plugin skills and optional MCP.

For PR repair, design all critical behavior around the shared substrate (`meta` CLI/MCP + `.meta.yaml` + worktrees), then make Claude and Codex front doors generated/verified views over that substrate.

---

## Accepted direction

Accepted after review: 2026-06-26

The chosen parity direction is:

> Make `meta` CLI/MCP the shared substrate, then generate and verify Claude and Codex front doors from one canonical source.

This means future PR repair and agentic workflow design should put durable semantics in `meta` commands, `meta-mcp`, `.meta.yaml`, plugin protocol, worktree metadata, and the selected state/knowledge layer. Claude slash commands and Codex prompts/skills should be generated or parity-checked views over that substrate, not independent sources of truth.

Operational rule until parity tooling exists:

```text
Codex and Claude agents must be given exact `rtk meta` / `meta` fallback commands in repair tickets. MCP tools are preferred when available, but CLI behavior is the required common denominator.
```
