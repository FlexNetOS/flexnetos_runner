# Agentic PR Failure Repair Queue v4 — Research Plan and Target Map

Created: 2026-06-26
Status: research-plan layer requested after v3 direction was accepted
Prior direction: `/home/drdave/.handoff/.idea/agentic-pr-failure-repair-queue.v3-meta-control-plane.md`
Original brainstorm: `/home/drdave/.handoff/.idea/agentic-pr-failure-repair-queue.md`

## Purpose

V3 established the direction: build the PR failure repair system as a thin supervisor layer over the existing `meta` control plane.

V4 adds the research plan needed before implementation, with special focus on:

1. how local `meta` skills/commands are loaded for Claude but not equivalently for Codex,
2. how to give Codex the same access and operational understanding,
3. how the existing `meta` CLI/plugin/workspace/worktree primitives should become the supervisor substrate,
4. how persistent knowledge/code intelligence should be represented,
5. how `icm`, `.kb`, `handoff`, `grit`, and Beads should fit into the queue/ledger/control loop.

This file is a plan, not an implementation.

## Thesis

The right path is still:

```text
meta-native PR repair supervisor
  + meta workspace graph
  + meta worktree repair lanes
  + meta exec verification
  + meta git publish/status
  + FlexNetOS runners
  + verifier/merge gate
  + persistent knowledge/code intelligence
```

But before building the supervisor, we should trace the already-built local systems that may supply most of the architecture:

- Claude command/skill loading,
- Codex plugin/skill loading,
- `meta` plugin dispatch,
- `meta` CLI and project graph,
- `loop_lib` execution model,
- `.kb` code intelligence,
- `handoff` workflow/ledger/task state,
- `icm` memory/database structure,
- `grit` multi-agent orchestration,
- Beads issue/task graph (`FlexNetOS/beads_rust`, `FlexNetOS/beads_viewer`).

## Early facts already verified

Local targets exist:

```text
/home/drdave/Desktop/meta/.kb
/home/drdave/Desktop/meta/.claude
/home/drdave/Desktop/meta/.codex
/home/drdave/Desktop/meta/meta-plugins
/home/drdave/Desktop/meta/meta_cli
/home/drdave/Desktop/meta/loop_lib
/home/drdave/Desktop/meta/handoff
/home/drdave/Desktop/meta/icm
/home/drdave/Desktop/meta/grit
```

Non-local but key target repos exist on GitHub:

```text
FlexNetOS/beads_rust
  description: Fast Rust port of Steve Yegge's beads: local-first, non-invasive issue tracker storing tasks in SQLite with JSONL export for git collaboration
  default branch: main

FlexNetOS/beads_viewer
  description: Graph-aware TUI for the Beads issue tracker: PageRank, critical path, kanban, dependency DAG visualization, and robot-mode JSON API
  default branch: main
```

## Research principles

1. **Do not invent where local systems already exist.**
2. **Trace loading paths before designing new commands.**
3. **Prefer durable state over chat context.**
4. **Separate global supervision from PR repair execution.**
5. **Use `meta` for workspace truth, not filesystem guessing.**
6. **Use code intelligence and persistent memory as first-class inputs.**
7. **Treat CI failures as structured learning events.**
8. **Keep Codex and Claude capability parity explicit.**

## Research Track A — Claude vs Codex skill/command loading

### Core question

How are `meta` skills/commands loaded for Claude, and why are they not automatically available to Codex with the same command surface?

### Targets

```text
/home/drdave/Desktop/meta/.claude
/home/drdave/Desktop/meta/.codex
/home/drdave/Desktop/meta/claude-plugins
/home/drdave/Desktop/meta/claude-plugin
/home/drdave/Desktop/meta/codex
/home/drdave/Desktop/meta/meta-plugins
/home/drdave/.codex/plugins/cache/gitkb/meta/*
/home/drdave/.codex/plugins/cache/gitkb/meta/0.1.0/skills/*
```

### Questions to answer

- Where do Claude slash commands live?
- Where do Claude skills load from?
- How does Claude discover `/meta:meta-git`, `/meta:meta-worktree`, `/meta:meta-workspace`, etc.?
- How does Codex discover plugin skills?
- What is missing from Codex compared to Claude?
- Are the differences:
  - naming only,
  - plugin manifest differences,
  - MCP server exposure,
  - command registry differences,
  - environment/path differences,
  - or product-surface limitations?
- Which files are canonical vs generated cache?
- What must be committed to repo/plugin source vs local-only user config?

### Desired output

A loading-path diagram:

```text
source repo / registry
  -> plugin package/cache
  -> skill/command manifest
  -> Claude surface
  -> Codex surface
  -> available slash/tool behavior
```

A parity checklist:

```yaml
meta_skill_parity:
  meta-git:
    claude: available_as_slash_command_or_skill
    codex: available_as_skill_only_or_manual
    gap: ...
    fix_path: ...
  meta-worktree:
    claude: ...
    codex: ...
```

## Research Track B — Meta plugins and command dispatch

### Core question

What is the `meta` plugin system's actual dispatch architecture, and where should the PR repair supervisor hook in?

### Targets

```text
/home/drdave/Desktop/meta/meta-plugins
/home/drdave/Desktop/meta/meta_plugin_api
/home/drdave/Desktop/meta/meta_plugin_protocol
/home/drdave/Desktop/meta/meta_cli
/home/drdave/Desktop/meta/meta_core
/home/drdave/Desktop/meta/meta_git_cli
/home/drdave/Desktop/meta/meta_git_lib
/home/drdave/Desktop/meta/meta_project_cli
/home/drdave/Desktop/meta/meta_mcp
/home/drdave/Desktop/meta/.meta/plugins
/home/drdave/Desktop/meta/.meta/meta-policy.json
```

### Questions to answer

- How does `meta` discover plugins?
- How are commands intercepted/enhanced?
- Where do `meta git`, `meta worktree`, `meta project`, and `meta exec` dispatch?
- Which command layer should host a future `meta pr-repair` or `meta repair` command?
- Should PR repair be:
  - a new `meta` plugin,
  - an `envctl` command,
  - a `handoff` verb,
  - a standalone script first,
  - or a GitHub App/runner dispatch capability?
- How does `meta_mcp` expose workspace state to agents?
- Is there already an MCP tool set that can serve supervisor state?

### Desired output

A control-plane integration decision:

```yaml
candidate_surfaces:
  meta_plugin:
    pros: native workspace graph, command dispatch, plugin architecture
    cons: requires plugin design/versioning
  envctl_command:
    pros: existing admin/secret/runtime control
    cons: may blur workspace-vs-env responsibilities
  handoff_verb:
    pros: task/ledger/workflow semantics
    cons: may not own git/worktree lifecycle
  standalone_script:
    pros: fastest proof
    cons: risks parallel system if not promoted
recommendation: ...
```

## Research Track C — `meta_cli`, `meta_core`, and project graph semantics

### Core question

What typed structures already represent workspace projects, tags, `provides`, `depends_on`, and execution ordering?

### Targets

```text
/home/drdave/Desktop/meta/meta_cli
/home/drdave/Desktop/meta/meta_core
/home/drdave/Desktop/meta/meta_project_cli
/home/drdave/Desktop/meta/.meta.yaml
```

### Questions to answer

- Where is `.meta.yaml` parsed?
- What Rust types represent a project?
- Are `provides` and `depends_on` first-class typed fields?
- Is topological/dependency order implemented?
- Can PR repair use existing impact analysis?
- Can the supervisor map GitHub repo name -> meta project key reliably?
- How are nested meta repos represented?
- How should missing/nonlocal repos be represented?

### Desired output

Typed model notes:

```yaml
workspace_model:
  project_type: <path>
  fields:
    name: ...
    path: ...
    repo: ...
    tags: ...
    provides: ...
    depends_on: ...
  repo_to_project_mapping: ...
  impact_analysis_available: true/false
```

## Research Track D — `loop_lib` execution and repair-runner model

### Core question

Can `loop_lib` provide the execution engine for a supervisor loop and bounded repair workflows?

### Targets

```text
/home/drdave/Desktop/meta/loop_lib
/home/drdave/Desktop/meta/loop_cli
/home/drdave/Desktop/meta/envctl
/home/drdave/Desktop/meta/handoff
```

### Questions to answer

- What loop abstractions already exist?
- Is there a durable state machine or iteration contract?
- Does it support:
  - retries,
  - step budgets,
  - watchdogs,
  - parallel lanes,
  - event logging,
  - failure classification?
- How does it differ from `handoff` loop/task semantics?
- Should PR repair supervisor be a loop_lib consumer?

### Desired output

```yaml
loop_capabilities:
  state_machine: yes/no
  retry_policy: yes/no
  durable_state: yes/no
  event_log: yes/no
  parallel_lanes: yes/no
  best_fit_for_pr_repair: ...
```

## Research Track E — `handoff` workflow, task state, and ledger

### Core question

Should PR repair tickets live in `handoff`, and what existing task/ledger semantics should be reused?

### Targets

```text
/home/drdave/Desktop/meta/handoff
/home/drdave/Desktop/meta/handoff/.handoff
/home/drdave/Desktop/meta/envctl/.handoff
/home/drdave/Desktop/meta/.handoff
```

### Questions to answer

- What is the canonical handoff task model?
- How are tasks, locks, status, and claims represented?
- What is the ledger database schema, if present?
- Does `handoff` already support queue-like work claims?
- How does handoff integrate with agents?
- Can a PR repair ticket be a handoff task?
- Should repair state be local-only, committed, or rollup-generated?

### Desired output

```yaml
handoff_fit:
  ticket_model_available: true/false
  claim_lock_model: true/false
  ledger_schema: ...
  recommended_role: source_of_repair_ticket_state / audit_log / not_primary
```

## Research Track F — `.kb` and code intelligence

### Core question

What code-intelligence database already exists in `meta/.kb`, and how can agents query it for accurate repair work?

### Targets

```text
/home/drdave/Desktop/meta/.kb
/home/drdave/Desktop/meta/.kb/*
/home/drdave/Desktop/meta/gitkb-related plugin/skills
/home/drdave/Desktop/meta/meta_mcp
```

### Questions to answer

- What database files exist?
- What schema/indexes exist?
- What entities are indexed?
- How are symbols, files, relationships, or summaries represented?
- Is it updated automatically?
- Which commands query it?
- Can repair agents ask:
  - what files own this failing function?
  - who depends on this crate/module?
  - what changed recently in this area?
  - what past failures mention this signature?
- Is `.kb` per-repo, workspace-wide, or both?

### Desired output

```yaml
code_intelligence:
  kb_path: /home/drdave/Desktop/meta/.kb
  db_files: []
  schema_summary: ...
  query_tools: []
  repair_agent_use_cases:
    - impact_analysis
    - symbol_lookup
    - failure_signature_lookup
    - ownership_mapping
```

## Research Track G — `icm` persistent memory and database structure

### Core question

How should ICM support PR repair learning, and what is its actual storage/query model?

### Targets

```text
/home/drdave/Desktop/meta/icm
/home/drdave/Desktop/meta/icm/src
/home/drdave/Desktop/meta/icm/Cargo.toml
ICM runtime data location discovered by `icm health` / config
```

### Questions to answer

- Where does ICM store data?
- What DB engine/schema does it use?
- What are topics, keys, links, importance levels?
- How are recall and recall-context implemented?
- Can recurring CI failure signatures be stored/retrieved reliably?
- Should PR repair write:
  - errors-resolved,
  - decisions,
  - context per repo,
  - failure-signatures,
  - preferences?
- How should memory avoid noise from transient CI logs?

### Desired output

```yaml
icm_role:
  durable_learning: true
  canonical_ticket_state: false/unknown
  best_topics:
    - errors-resolved
    - decisions-pr-repair
    - context-<repo>
    - failure-signatures
  storage_schema: ...
```

## Research Track H — `grit` multi-agent orchestration

### Core question

Is `meta/grit` the existing agent orchestration substrate for multi-agent PR repair?

### Targets

```text
/home/drdave/Desktop/meta/grit
/home/drdave/Desktop/meta/grit/README.md
/home/drdave/Desktop/meta/grit/Cargo.toml
/home/drdave/Desktop/meta/grit/src
```

### Questions to answer

- What does `grit` currently orchestrate?
- Does it manage parallel agents?
- Does it have task assignment, message passing, or role routing?
- Does it integrate with GitHub, worktrees, or runners?
- Is it a better home for supervisor/repair agent assignment than `handoff` or `meta`?
- How does it relate to `atc`, `weave`, `harness-agent-rs`, and `loop_lib`?

### Desired output

```yaml
grit_role:
  possible_roles:
    - agent_assignment
    - repair_worker_runtime
    - supervisor_runtime
    - not_in_scope
  evidence: ...
  integration_path: ...
```

## Research Track I — Beads task graph (`beads_rust`, `beads_viewer`)

### Core question

Should Beads provide the issue/task graph for the PR repair queue?

### Targets

Nonlocal repos:

```text
FlexNetOS/beads_rust
FlexNetOS/beads_viewer
```

Known descriptions:

```text
beads_rust:
  Fast Rust port of Steve Yegge's beads: local-first, non-invasive issue tracker storing tasks in SQLite with JSONL export for git collaboration.

beads_viewer:
  Graph-aware TUI for the Beads issue tracker: PageRank, critical path, kanban, dependency DAG visualization, and robot-mode JSON API.
```

### Questions to answer

- What is Beads' SQLite schema?
- What is the JSONL export/import format?
- Does it support task dependencies, priorities, claims, status, assignees?
- Can it represent PR repair tickets and shared incidents?
- Does `beads_viewer` robot-mode JSON API provide a ready supervisor dashboard/API?
- How does Beads compare to `handoff` ledger for this use case?
- Should Beads be cloned into `/home/drdave/Desktop/meta` and registered in `.meta.yaml`?
- Is Beads the best canonical queue while handoff/ICM provide context/learning?

### Desired output

```yaml
beads_fit:
  local_first_queue: likely
  sqlite_backing: yes
  git_jsonl_sync: yes
  graph_prioritization: likely via beads_viewer
  possible_role: canonical_pr_repair_ticket_graph
  open_questions:
    - integration_with_handoff
    - integration_with_meta_worktree_metadata
    - sync/commit policy
```

## Research Track J — FlexNetOS runners and GitHub App integration

### Core question

How should `flexnetos_runner` and `flexnetos_github_app` participate once the supervisor exists?

### Targets

```text
/home/drdave/Desktop/meta/flexnetos_runner
/home/drdave/Desktop/meta/flexnetos_github_app
/home/drdave/Desktop/meta/envctl
```

### Questions to answer

- Should the supervisor be triggered by:
  - cron/polling,
  - GitHub webhook through `flexnetos_github_app`,
  - manual command,
  - runner workflow_dispatch,
  - handoff loop?
- Should repair agents run as GitHub Actions jobs on self-hosted runners, or as local sessions managed outside Actions?
- How should secrets be minted and scoped?
- How should runner capacity limit active repair lanes?
- How should the runner evaluator feed metrics into supervisor decisions?

### Desired output

```yaml
execution_plane:
  trigger_options:
    webhook: ...
    poller: ...
    manual: ...
  repair_worker_runtime: local_agent_or_actions_job
  capacity_policy: active_ci_jobs <= runner_count
  metrics_input: eval-runners artifacts and GitHub run timings
```

## Research deliverables

### Deliverable 1 — Loading parity map

A document that explains exactly how meta skills load for Claude and Codex, with the path to parity.

Suggested path:

```text
/home/drdave/.handoff/.idea/meta-skill-loading-parity-map.md
```

### Deliverable 2 — Control-plane integration map

A document mapping PR repair responsibilities to existing repos/commands.

Suggested path:

```text
/home/drdave/.handoff/.idea/pr-repair-control-plane-map.md
```

### Deliverable 3 — Persistent state decision

A decision record comparing:

- `meta worktree` metadata,
- `.handoff` / handoff ledger,
- ICM,
- Beads SQLite/JSONL,
- GitHub issues/PR comments.

Suggested path:

```text
/home/drdave/.handoff/.idea/pr-repair-state-decision.md
```

### Deliverable 4 — Prototype command design

A lightweight command/API sketch, for example:

```bash
meta pr-repair scan
meta pr-repair classify --repo FlexNetOS/meta --pr 66
meta pr-repair lane create FlexNetOS/meta#66
meta pr-repair assign repair-meta-pr-66 --agent codex
meta pr-repair watch
meta pr-repair merge-green
```

Suggested path:

```text
/home/drdave/.handoff/.idea/meta-pr-repair-command-sketch.md
```

## Recommended research order

1. **Trace skill loading parity**
   - `.claude`, `.codex`, meta plugins, Codex plugin cache.
   - Goal: same understanding/access for Codex as Claude.

2. **Trace `meta` command/control architecture**
   - `meta_cli`, `meta_core`, `meta-plugins`, `meta_mcp`.
   - Goal: determine best home for `pr-repair` supervisor surface.

3. **Trace `meta worktree` implementation**
   - especially `--from-pr`, `--meta`, `--ttl`, `~/.meta/worktree.json`.
   - Goal: confirm repair-lane mechanics.

4. **Trace `loop_lib` and `handoff`**
   - Goal: decide whether supervisor loop/task state belongs in existing loop/handoff abstractions.

5. **Trace `.kb` and ICM**
   - Goal: make code intelligence and learning first-class.

6. **Trace `grit`**
   - Goal: determine whether it should run/assign repair agents.

7. **Clone or inspect Beads repos**
   - Goal: decide whether Beads is the canonical PR repair queue/task graph.

8. **Synthesize v5 implementation plan**
   - No implementation until the above is mapped.

## Hypothesis to validate

The likely best architecture is:

```text
meta pr-repair supervisor
  uses .meta.yaml as fleet graph
  uses meta worktree as lane manager
  uses Beads or handoff as ticket graph/ledger
  uses ICM for memory and repeated failure signatures
  uses .kb/GitKB for code intelligence
  uses grit/weave/atc for agent assignment/runtime if mature enough
  uses FlexNetOS runners for CI execution and metrics
  uses GitHub App/gh for PR/check mutation
```

## Important caution

Do not prematurely build a new queue database or new agent runtime until Beads, handoff, ICM, `.kb`, and grit are inspected. There is a high chance one or more of them already implements the needed primitives.

The correct move is to connect existing systems, not duplicate them.

---

## Accepted discovery and recommendation — Meta CLI/MCP as shared substrate

Accepted: 2026-06-26

The parity research confirmed the direction from v3/v4 and sharpens it into an implementation rule:

> Make `meta` CLI/MCP the shared substrate, then generate and verify Claude and Codex front doors from one canonical source.

### Discovery

Claude and Codex both have access to `meta` concepts, but not through the same loading path:

- Claude has a first-class `meta init claude` path that installs `.claude/skills`, `.claude/rules`, and `.claude/settings.json` hooks.
- Claude also has richer repo-local slash-command affordances under `.claude/commands`.
- Codex has project `.codex` hooks/prompts/agents, a configured `meta-mcp`, and an installed `meta@gitkb` plugin cache under `~/.codex/plugins/cache/gitkb/meta/...`.
- Codex can see the meta plugin skills, but not as a full Claude-style command surface.
- Direct `meta_*` MCP tools may be configured but are not guaranteed to be surfaced in every active Codex session, so CLI fallback remains mandatory.
- The same core meta skill guidance exists in multiple places and is drifting: `.claude/skills`, `claude-plugin/skills`, Codex plugin cache, and embedded `meta_cli` init sources are not byte-identical.

### Recommendation

The PR repair supervisor and future agentic workflow must not depend on Claude-only or Codex-only front doors as the source of truth.

Use this stack as the durable substrate:

```text
.meta.yaml
meta CLI
meta MCP
meta plugin protocol
meta worktree metadata
GitHub PR/check state
FlexNetOS runners
handoff / ICM / Beads / .kb as state and knowledge layers after their research is complete
```

Then treat Claude and Codex surfaces as generated or verified projections:

```text
canonical meta agent instructions
  -> Claude commands / skills / hooks
  -> Codex prompts / skills / hooks / agents
  -> MCP tool docs and fallback command recipes
```

### Implementation implications

Future implementation should include or design:

1. `meta init codex` or equivalent generation support.
2. A parity verifier for `.claude`, `.codex`, plugin cache, and MCP availability.
3. Canonical meta skill/prompt source selection.
4. Matching Claude and Codex front doors for PR repair.
5. Every Codex repair ticket must include explicit CLI fallback commands, even if MCP is expected.

### Working rule for PR repair agents

Until parity tooling exists, every repair worker prompt should include:

```text
Start from /home/drdave/Desktop/meta.
Run `rtk meta project list --json` and `rtk meta git status`.
Use `rtk meta` / `meta` command-line operations as the reliable path.
Use meta MCP tools only if exposed in the session.
Treat Claude/Codex prompt or slash-command surfaces as convenience front doors, not as canonical state.
```

### Updated architectural decision

The supervisor should be built as a meta-native control loop. Claude and Codex should operate it through equivalent, generated/verified front doors, but the semantics must live underneath them in the shared `meta` substrate.
