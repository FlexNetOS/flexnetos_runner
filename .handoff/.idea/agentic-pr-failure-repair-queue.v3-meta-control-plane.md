# Agentic PR Failure Repair Queue v3 — Meta Control Plane Direction

Created: 2026-06-26
Status: direction accepted in chat
Canonical prior context: `/home/drdave/.handoff/.idea/agentic-pr-failure-repair-queue.md`

## One-line direction

Build the PR failure repair system as a **thin supervisor layer over the existing `meta` control plane**, not as a parallel orchestration system.

`meta project list` is the fleet graph. `meta worktree` is the repair-lane primitive. `meta exec` is the scoped verification runner. `meta git` is the fleet state/publish surface. The self-hosted FlexNetOS runners are the execution plane.

## What changed from v1/v2

The first brainstorm correctly identified the need for:

- a supervisor/orchestrator,
- per-PR repair tickets,
- fresh repair agents by default,
- original/live session use only when ownership is clear,
- verifier-controlled merge,
- stale-check reruns,
- repeated-failure incidents,
- durable learning.

The v3 refinement is that we do **not** need to invent the central control structure. It already exists locally in the `meta` workspace and plugin/command system.

The PR repair queue should therefore be a small orchestration layer that uses existing `meta` primitives rather than competing with them.

## Why this is the lowest-confusion path

One global source of truth already exists:

- `/home/drdave/Desktop/meta/.meta.yaml`
- `meta project list --json`
- `~/.meta/worktree.json` for worktree metadata
- `meta git status` for fleet state
- GitHub PR/check state for remote truth
- FlexNetOS runner API/systemd for execution-plane truth

This gives every unit of work a clear place:

| Concern | Existing substrate |
|---|---|
| Fleet/repo graph | `.meta.yaml`, `meta project list --json` |
| Repo tags and dependency metadata | `.meta.yaml` tags, `provides`, `depends_on` |
| Isolated PR repair lane | `meta worktree create --from-pr ...` |
| Lane metadata | `meta worktree --meta key=value`, `~/.meta/worktree.json` |
| Fleet status | `meta git status` |
| Scoped command execution | `meta exec`, `--include`, `--tag`, `--ordered`, `--parallel` |
| Safe batch state | `meta git snapshot create/restore` |
| CI/job truth | `gh pr checks`, `gh run view`, runner API |
| Execution plane | FlexNetOS self-hosted runners |

That removes communication ambiguity. A PR repair is not an informal chat thread; it is a named worktree lane with metadata, a PR URL, an owning agent, and a state.

## Existing local primitives found

### `/meta:meta-workspace`

Use this to discover the world at session/supervisor start:

```bash
meta project list --json
meta git status
```

The workspace map includes repo paths, remotes, tags, and `is_meta` markers. `.meta.yaml` also carries the higher-value dependency fields:

- `provides`
- `depends_on`
- tags such as `canon`, `ai`, `orchestration`, `ops`, `runner`, `tools`, `mcp`

This means the supervisor can classify impacted repos and prioritize repairs without scraping the filesystem manually.

### `/meta:meta-git`

Use this for fleet state and publish discipline:

```bash
meta git status
meta git update
meta git snapshot create <name>
meta --include <repo> git status
meta --include <repo> git push
```

The owner rule still stands: every committed chunk gets committed, pushed, and PR'd immediately. `meta git` gives a fleet-aware way to inspect and publish without missing repos.

### `/meta:meta-worktree`

This is the core repair-lane primitive.

Relevant existing features:

- isolated worktree sets,
- `--from-pr org/repo#N`,
- `--ephemeral`,
- `--ttl`,
- `--meta key=value`,
- `meta worktree list --json`,
- `meta worktree status <name>`,
- `meta worktree exec <name> ...`,
- centralized metadata at `~/.meta/worktree.json`.

A failed PR should become a worktree lane like:

```bash
meta worktree create repair-meta-66 \
  --from-pr FlexNetOS/meta#66 \
  --repo . \
  --repo meta \
  --ephemeral \
  --ttl 6h \
  --meta kind=pr-repair \
  --meta repo=FlexNetOS/meta \
  --meta pr=66 \
  --meta owner=repair-agent-01 \
  --meta source=pr-failure-supervisor
```

Important note: include `--repo .` when possible so the repair lane has the root meta config and full command/tag behavior.

### `/meta:meta-exec`

Use this for scoped validation:

```bash
meta --include meta exec -- cargo test
meta --tag canon --ordered exec -- cargo build
meta --dry-run exec -- dangerous-command
meta --json exec -- git rev-parse HEAD
```

The repair agent should run the narrowest reproducer first, then required gates. Use `--ordered` for dependency-aware builds and `--parallel` only when dependencies do not require ordering.

### `/meta:meta-safety`

Use this as the safety discipline:

- start with `meta project list --json`,
- run `meta git status`,
- target exact repos with `--include`/`--tag`,
- inspect dependency impact before modifying shared providers,
- use `meta --ordered exec` when dependency order matters,
- use dry runs for broad/dangerous commands.

## Proposed v3 architecture

```text
GitHub PR/check state
        |
        v
+---------------------------+
| PR repair supervisor      |
| - poll PRs/checks/runs    |
| - classify failures       |
| - create repair lanes     |
| - assign agents           |
| - monitor CI              |
| - merge green PRs         |
+-------------+-------------+
              |
              | uses existing meta control plane
              v
+---------------------------+      +------------------------+
| meta workspace graph      |      | meta worktree lanes    |
| .meta.yaml                | ---> | repair-<repo>-<pr>     |
| project list/json         |      | ttl/meta/from-pr       |
+---------------------------+      +-----------+------------+
                                                  |
                                                  v
                                      +------------------------+
                                      | repair agent           |
                                      | - inspect logs         |
                                      | - patch scoped branch  |
                                      | - validate             |
                                      | - commit/push/update PR|
                                      +-----------+------------+
                                                  |
                                                  v
                                      +------------------------+
                                      | FlexNetOS runners      |
                                      | GitHub Actions CI      |
                                      +-----------+------------+
                                                  |
                                                  v
                                      +------------------------+
                                      | verifier/merge gate    |
                                      | - checks green         |
                                      | - scope clean          |
                                      | - merge/automerge      |
                                      +------------------------+
```

## Supervisor responsibilities

The supervisor should be the only component that owns the global queue.

Responsibilities:

1. Load fleet graph:

   ```bash
   meta project list --json
   ```

2. Inspect fleet state:

   ```bash
   meta git status
   meta worktree list --json
   ```

3. Poll PR/check state:

   ```bash
   gh search prs --owner FlexNetOS --state open
   gh pr view <n> --repo <repo> --json statusCheckRollup,mergeStateStatus,headRefName
   gh run view <run> --repo <repo> --json jobs,status,conclusion
   ```

4. Classify failures.
5. Rerun stale checks when appropriate.
6. Create one repair lane per real failed PR.
7. Assign an agent.
8. Monitor CI after pushes.
9. Merge green PRs.
10. Store lessons/failure signatures.

## Repair ticket schema

A queue record can be a JSONL row, DB row, or worktree metadata projection.

```json
{
  "kind": "pr-repair",
  "schema": "flexnetos.pr_repair.v1",
  "repo": "FlexNetOS/meta",
  "project": "meta",
  "pr": 66,
  "title": "ci: runner-smoke probe — verify self-hosted fxrun runners",
  "head_ref": "smoke/runner-verify",
  "base_ref": "main",
  "state": "classified",
  "classification": "mixed_stale_and_real_failure",
  "failing_checks": ["Test (windows-latest)", "Integration Tests (ubuntu-latest)"],
  "queued_checks": ["Clippy", "Integration Tests (ubuntu-latest)"],
  "runner_status": "healthy",
  "repair_lane": "repair-meta-66",
  "assigned_agent": "repair-agent-01",
  "recommended_action": "rerun stale queued checks, then patch real failures",
  "created_at": "2026-06-26T23:30:00Z",
  "updated_at": "2026-06-26T23:30:00Z"
}
```

## Failure classifications

```yaml
classifications:
  stale_queued:
    meaning: queued before runner access/infra was fixed
    action: rerun_or_cancel_and_rerun

  runner_infra:
    meaning: runner offline, wrong labels, repo not selected in runner group, service broken
    action: fix_runner_infra_before_touching_code

  merge_conflict:
    meaning: PR branch cannot merge cleanly
    action: update_branch_or_rebase_in_lane

  test_regression:
    meaning: failure appears caused by PR code
    action: create_repair_lane_and_assign_agent

  platform_specific:
    meaning: failure isolated to windows/macos/linux
    action: route_to_platform_lane

  repeated_cross_pr_failure:
    meaning: same failure appears across many unrelated PRs
    action: open_shared_incident_or_base_fix_lane

  policy_failure:
    meaning: semantic title, formatting, generated file policy, permissions, dependency-review, etc.
    action: patch_metadata_or_policy_scope_only

  superseded:
    meaning: branch is stale or replaced by newer PR
    action: close_or_regenerate
```

## Agent assignment rule

Use the original/live session only when all are true:

1. It created the PR.
2. It still owns the branch/worktree.
3. It is alive/reachable.
4. The failure is likely related to its change.

Otherwise spawn a clean repair agent in a `meta worktree` repair lane.

This is the least-confusing communication rule.

## Repair lane naming convention

Recommended lane name:

```text
repair-<project-key>-pr-<number>
```

Examples:

```text
repair-meta-pr-66
repair-envctl-pr-267
repair-handoff-pr-179
```

Use metadata to avoid ambiguity:

```bash
--meta kind=pr-repair
--meta repo=FlexNetOS/meta
--meta project=meta
--meta pr=66
--meta check='Integration Tests (ubuntu-latest)'
--meta owner=repair-agent-01
--meta source=pr-failure-supervisor
```

## Repair agent prompt template

```text
You are the repair agent for <repo> PR #<number>: <title>.

Repair lane:
- worktree: <worktree-name>
- project key: <project>
- branch: <head_ref>
- base: <base_ref>

Goal:
Make this PR mergeable without expanding scope.

Inputs:
- PR URL: <url>
- Failing checks: <checks>
- Relevant logs: <links or excerpts>
- Suspected classification: <classification>

Rules:
- Work only inside the assigned meta worktree lane.
- Inspect current branch and CI logs before editing.
- Patch only the PR's intended scope unless the failure is proven shared infra.
- Run the narrow reproducer first, then required gates.
- Use meta commands for workspace-aware operations.
- Commit and push every completed chunk immediately.
- Update the PR with root cause, fix, and validation.
- Stop only when CI is green, PR is superseded/closed, or blocker is explicit and evidenced.

Expected output:
- root cause
- files changed
- validation run
- pushed commit(s)
- PR/check status
- next action if not green
```

## Suggested command flow for one PR repair

```bash
# Supervisor creates lane
meta worktree create repair-meta-pr-66 \
  --from-pr FlexNetOS/meta#66 \
  --repo . \
  --repo meta \
  --ephemeral \
  --ttl 6h \
  --meta kind=pr-repair \
  --meta repo=FlexNetOS/meta \
  --meta pr=66

# Agent enters lane and inspects
cd /home/drdave/Desktop/meta/.worktrees/repair-meta-pr-66/meta
meta project list --json
meta git status

# Agent runs targeted checks
meta --include meta exec -- <targeted-check>

# Agent patches, validates, commits, pushes
meta --include meta git status
git add <files>
git commit -m "fix: ..."
git push

# Supervisor/verifier monitors
GH_FORCE_TTY=0 gh pr checks 66 --repo FlexNetOS/meta --watch
```

## Recommended immediate implementation order

1. **Manual supervisor mode using existing commands**
   - No new code yet.
   - Use `gh`, `meta project list`, `meta worktree`, `meta git`, and runner evaluator manually.
   - Prove the workflow on 2-3 failed PRs.

2. **Thin script wrapper**
   - Script creates queue records and worktree lanes.
   - Script does not repair code.
   - It only classifies, creates lanes, and emits prompts.

3. **Agent assignment integration**
   - Route tickets to live sessions when ownership is clear.
   - Otherwise spawn clean repair workers.

4. **Verifier/merge automation**
   - Watch checks.
   - Merge green PRs.
   - Close/regenerate superseded PRs.

5. **Durable learning**
   - Store repeated failure signatures in ICM and/or `.handoff/pr-repair/lessons`.

## What to avoid

- Do not build a new repo registry; use `.meta.yaml`.
- Do not build a new worktree manager; use `meta worktree`.
- Do not manually cd through repos when `meta` can target them.
- Do not assign the same PR to multiple agents.
- Do not let repair agents operate outside their lane.
- Do not revive stale sessions when a clean repair lane is safer.
- Do not merge red PRs unless failures are explicitly understood and non-blocking.
- Do not run broad expensive checks when a targeted reproducer exists.

## Why this path gives least confusion, highest quality, and speed

### Least confusion

- One queue owner: supervisor.
- One lane per PR: `meta worktree`.
- One agent per lane.
- One remote PR as the external truth.
- One verifier controls merge.

### Highest code quality

- Clean isolated worktree per PR.
- PR-scope-only patches.
- Test-first repair.
- Dependency-aware validation through `meta --ordered` when needed.
- CI verification before merge.

### Highest speed

- Parallel repair lanes.
- Existing runners execute CI.
- `meta` filters avoid wasted work.
- Stale checks are rerun before debugging.
- Shared failures become incidents instead of duplicated investigations.

## How this aligns with kclaw0-style learning

The kclaw0 reference points toward co-directional learning/self-update: the system should improve from feedback.

In v3, every failed PR is feedback:

- ticket classification improves routing,
- repeated failure signatures become memory,
- successful repairs become playbooks,
- stale/noisy checks become policy knowledge,
- the supervisor gets better at choosing original session vs clean repair agent.

This turns CI failure from interruption into training data for the workflow.

## Open design questions

1. Should queue state live first in:
   - `.handoff/pr-repair/queue.jsonl`,
   - `~/.meta/worktree.json` metadata only,
   - envctl/handoff ledger,
   - or all of the above with one canonical source?

2. Should repair tickets be GitHub issues/comments, local ledger rows, or both?

3. What concurrency cap should the supervisor enforce relative to runner count?
   - likely `active_repair_ci_jobs <= runner_count`, with local analysis allowed beyond that.

4. Which checks are required vs advisory per repo?

5. Should platform lanes be explicit?
   - `linux-repair`, `windows-repair`, `macos-repair`.

6. How should original live sessions be discovered and messaged?
   - Weave sessions?
   - handoff ledger?
   - PR metadata?
   - branch/worktree ownership metadata?

## Bottom-line decision

Implement the full agentic PR repair workflow as:

```text
meta-native supervisor
  + meta workspace graph
  + meta worktree repair lanes
  + meta exec validation
  + meta git publish/status
  + FlexNetOS runners
  + verifier/merge gate
  + ICM/ledger learning
```

This preserves the v1 brainstorm while anchoring the implementation in the existing local control plane.
