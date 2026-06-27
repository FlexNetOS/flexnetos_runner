# Meta PR Repair Command Sketch

Created: 2026-06-27
Parent idea: `.handoff/.idea/agentic-pr-failure-repair-queue.v5-control-plane-map.md`
Status: command/API sketch for first prototype

## Placement recommendation

Start as a `meta` subprocess plugin or thin script that shells to existing stable commands, then
promote once the interface is proven. The prototype must use the current worktree command surface:

```bash
rtk meta git worktree ...
```

Top-level `meta worktree` and `meta pr-repair` can be future aliases/front doors after parity work.

## Common options

All commands should support:

```text
--json       machine-readable output
--dry-run    print planned mutations without changing GitHub/worktrees/files
--repo       filter OWNER/REPO or meta project alias
--state      filter queue state
--limit      cap PR/API work
--since      time filter for scans/runs
```

All mutating commands should be dry-run capable where practical.

## `scan`

Purpose: discover open PRs and summarize check state without creating lanes.

```bash
meta pr-repair scan --owner FlexNetOS --json
```

JSON shape:

```json
{
  "schema": "flexnetos.pr_repair.scan.v1",
  "generated_at": "2026-06-27T00:00:00Z",
  "runner_health": {
    "status": "healthy",
    "online": 2,
    "busy": 0,
    "group_coverage": "86/86"
  },
  "prs": [
    {
      "id": "FlexNetOS/meta#66",
      "repo": "FlexNetOS/meta",
      "project": "meta",
      "number": 66,
      "title": "...",
      "url": "...",
      "head_ref": "...",
      "base_ref": "main",
      "state": "open",
      "checks": {
        "success": 3,
        "failure": 1,
        "queued": 0,
        "in_progress": 0
      },
      "suggested_classification": "pr_test_regression"
    }
  ]
}
```

CLI fallback for prototype:

```bash
gh search prs --owner FlexNetOS --state open --json repository,number,title,url,headRefName,updatedAt
```

MCP equivalent when available: use GitHub/metadata tools for PRs and `meta_workspace_state` for repo
mapping. CLI remains required fallback.

## `classify`

Purpose: turn one PR into a repair decision.

```bash
meta pr-repair classify FlexNetOS/meta#66 --json
```

JSON shape:

```json
{
  "schema": "flexnetos.pr_repair.classification.v1",
  "id": "FlexNetOS/meta#66",
  "classification": "pr_test_regression",
  "confidence": "medium",
  "evidence": [
    "check Test (ubuntu-latest) failed after runner pickup",
    "runner group coverage healthy"
  ],
  "recommended_action": "create_lane",
  "rerun_first": false,
  "lane_name": "repair-meta-pr-66"
}
```

Classification enum:

```text
stale_queued
runner_infra
merge_conflict
pr_test_regression
platform_specific
cross_pr_shared_failure
policy_or_metadata_failure
superseded
flaky
unknown
```

## `lane create`

Purpose: create an isolated repair worktree and attach metadata.

```bash
meta pr-repair lane create FlexNetOS/meta#66 --dry-run --json
```

Prototype implementation should generate and run a command like:

```bash
rtk meta git worktree create repair-meta-pr-66 \
  --from-pr FlexNetOS/meta#66 \
  --repo . --repo meta \
  --ephemeral --ttl 6h \
  --meta kind=pr-repair \
  --meta repo=FlexNetOS/meta \
  --meta pr=66 \
  --meta state=classified \
  --meta classification=pr_test_regression
```

Dry-run JSON:

```json
{
  "schema": "flexnetos.pr_repair.lane_plan.v1",
  "dry_run": true,
  "lane": "repair-meta-pr-66",
  "commands": [
    "rtk meta git worktree create ..."
  ],
  "would_write_queue": true,
  "would_create_worktree": true
}
```

## `assign`

Purpose: produce a worker prompt and mark the lane assigned.

```bash
meta pr-repair assign repair-meta-pr-66 --agent codex --json
```

Output should include:

- lane path,
- PR URL,
- failing checks/log links,
- exact startup commands,
- validation commands,
- allowed scope,
- stop condition.

For v1, write prompt projection to:

```text
.handoff/pr-repair/agents/<repo>-<pr>-assignment.md
```

## `watch`

Purpose: monitor lanes and PR checks.

```bash
meta pr-repair watch --json
```

JSON shape:

```json
{
  "schema": "flexnetos.pr_repair.watch.v1",
  "lanes": [
    {
      "lane": "repair-meta-pr-66",
      "repo": "FlexNetOS/meta",
      "pr": 66,
      "state": "awaiting_ci",
      "checks": {"failure": 0, "pending": 1, "success": 5},
      "next_action": "wait"
    }
  ]
}
```

## `merge-green`

Purpose: merge or arm automerge for lanes that satisfy verifier policy.

```bash
meta pr-repair merge-green --dry-run --json
```

Verifier gates:

- PR is open and not draft.
- Required checks are green or automerge can be armed.
- No blocking review.
- Lane diff is scoped to intended PR.
- No unrelated dirty state remains.

Planned merge command:

```bash
gh pr merge <PR> --repo <OWNER/REPO> --auto --squash --delete-branch
```

## Implementation home decision

Recommended sequence:

1. Script/prototype first, stored near the chosen meta plugin or as a repo-local proof.
2. Promote to a `meta` subprocess plugin after manual supervisor proof.
3. Add top-level `meta pr-repair` once command semantics are stable.
4. Generate Claude/Codex front doors from the same command docs.

Do not implement direct agent spawning until scan/classify/lane/watch/merge-green are proven with
manual workers.
