# PR Repair Control Plane Map

Created: 2026-06-27
Parent idea: `.handoff/.idea/agentic-pr-failure-repair-queue.v5-control-plane-map.md`
Status: design artifact / current-state corrected map

## Executive summary

The PR repair supervisor should be meta-native, but current repo truth corrects one important V5
assumption: the worktree surface is currently exposed as `meta git worktree`, not top-level
`meta worktree`. The `meta` plugin dispatcher source mentions top-level plugin command exposure, and
Claude skill text documents `meta worktree`, but the live CLI in this checkout accepts:

```bash
rtk meta git worktree <command>
```

Therefore the v1 implementation should use `meta git worktree` as the reliable command surface and
record top-level `meta worktree` as a parity/alias follow-up, not as a prerequisite.

## Control-plane responsibilities

| Responsibility | Current reliable substrate | Notes |
|---|---|---|
| Fleet graph | `rtk meta project list --json` | Use `.meta.yaml`-backed project data; no filesystem guessing. |
| Fleet dirty state | `rtk meta git status` | Supervisor must start here before assigning lanes. |
| PR/check scan | `gh pr list/view`, `gh run list/view`, GitHub API | GitHub remains remote truth for checks, mergeability, review state. |
| Runner health | `scripts/eval-runners.sh`, org runner API, systemd | Only classify as infra failure with live evidence. |
| Repair lane | `rtk meta git worktree create/list/status/exec/diff/remove/prune` | Use `--from-pr`, `--meta`, `--ttl`, `--ephemeral`, and `--repo .` when possible. |
| Queue/audit | handoff ledger or `.handoff/pr-repair` projection | See state decision. |
| Learning | ICM + optional `.handoff/pr-repair/lessons` view | Repeated signatures are memory, not chat-only notes. |
| Code targeting | GitKB / `.kb` and repo search | Worker repair prompts should include code-intelligence commands when known. |
| Publish/merge | git/gh/meta git + PR checks | Owner rule: committed chunk means push + PR immediately. |

## Supervisor loop

```text
scan -> classify -> refresh stale -> create/update lane -> assign worker
     -> watch CI -> verify scope/checks -> merge/automerge -> store lesson
```

### Scan

Minimum manual scan:

```bash
cd /home/drdave/Desktop/meta
rtk meta project list --json
rtk meta git status
rtk meta git worktree list --json
gh search prs --owner FlexNetOS --state open --json repository,number,title,url,headRefName,updatedAt
```

For each candidate PR:

```bash
gh pr view <PR> --repo <OWNER/REPO> \
  --json number,title,url,state,isDraft,mergeStateStatus,headRefName,baseRefName,statusCheckRollup,reviews
```

### Classify

Classify before creating a worker lane.

| Classification | Evidence | Action |
|---|---|---|
| `stale_queued` | check queued before runner-group repair or no runner pickup evidence | rerun once; do not assign worker yet |
| `runner_infra` | runner offline/wrong label/repo absent from runner group/service broken | fix infra first |
| `merge_conflict` | GitHub mergeability or local rebase conflict | update branch lane |
| `pr_test_regression` | failing test tied to PR diff | assign repair worker |
| `platform_specific` | only Windows/macOS/Linux fails | assign platform lane or classify as advisory if non-required |
| `cross_pr_shared_failure` | same signature across multiple PRs | create incident/base-fix lane |
| `policy_or_metadata_failure` | semantic title, workflow permission, action config | patch metadata/policy only |
| `superseded` | newer PR/branch replaces it | close or regenerate |
| `flaky` | rerun succeeds or signature known flaky | record signature; rerun policy only |

### Lane lifecycle

```text
none
  -> discovered        # PR observed by scan
  -> classified        # classification and recommended action exists
  -> lane_created      # worktree exists with metadata
  -> assigned          # exactly one worker/session owns it
  -> repairing         # worker has made or is testing changes
  -> awaiting_ci       # worker pushed, CI pending
  -> green             # required checks green
  -> merged            # PR merged/automerge completed
  -> blocked           # evidenced blocker, no blind retries
  -> superseded        # closed/replaced by newer work
```

A lane name should be deterministic and shell-safe:

```text
repair-<project>-pr-<number>
```

Example:

```bash
rtk meta git worktree create repair-meta-pr-66 \
  --from-pr FlexNetOS/meta#66 \
  --repo . \
  --repo meta \
  --ephemeral \
  --ttl 6h \
  --meta kind=pr-repair \
  --meta repo=FlexNetOS/meta \
  --meta pr=66 \
  --meta state=classified \
  --meta classification=pr_test_regression \
  --meta source=pr-repair-supervisor
```

Current `meta git worktree create --help` confirms these relevant options exist:

- `--from-pr <OWNER/REPO#N>`
- `--repo <ALIAS[:BRANCH]>`
- `--ephemeral`
- `--ttl <DURATION>`
- `--meta <KEY=VALUE>`
- `--dry-run`
- `--strict`
- `--no-deps`
- `--recursive`

Current `meta git worktree list --json` emits entries with `name`, `root`, `has_meta_root`, `repos`,
`ephemeral`, optional `ttl_remaining_seconds`, and optional `custom` metadata. The Rust store type
persists custom metadata in `WorktreeStoreEntry.custom` at `~/.meta/worktree.json`.

### Worker prompt contract

Every worker prompt must be surface-neutral and include command fallbacks:

```text
You are the repair worker for <OWNER/REPO>#<PR>.

Start:
cd /home/drdave/Desktop/meta
rtk meta project list --json
rtk meta git status
rtk meta git worktree status <lane>

Scope:
- Work only inside <lane path>.
- Patch only the PR's intended scope unless supervisor reclassifies as shared incident.
- Commit, push, and update PR for every completed chunk.

Stop when:
- required CI is green and PR is mergeable,
- PR is superseded/closed,
- or blocker is evidenced with logs and exact next action.
```

### Verifier / merge gate

Verifier checks before merge/automerge:

```bash
rtk meta git worktree status <lane>
rtk meta git worktree diff <lane> --base <base>
gh pr checks <PR> --repo <OWNER/REPO> --watch
gh pr view <PR> --repo <OWNER/REPO> --json mergeStateStatus,isDraft,reviewDecision,statusCheckRollup
```

Merge only when:

- branch diff matches intended scope,
- required checks are green,
- no blocking review remains,
- no unrelated dirty state remains,
- and PR is not superseded.

## Exact manual command sequence for one PR

```bash
cd /home/drdave/Desktop/meta

# 1. Fleet and runner preflight
rtk meta project list --json
rtk meta git status
rtk meta git worktree list --json

# 2. Inspect one PR
gh pr view 66 --repo FlexNetOS/meta \
  --json number,title,url,state,isDraft,mergeStateStatus,headRefName,baseRefName,statusCheckRollup

# 3. Rerun stale checks if classification is stale_queued
gh run rerun <RUN_ID> --repo FlexNetOS/meta

# 4. Create lane for real failure
rtk meta git worktree create repair-meta-pr-66 \
  --from-pr FlexNetOS/meta#66 \
  --repo . --repo meta \
  --ephemeral --ttl 6h \
  --meta kind=pr-repair \
  --meta repo=FlexNetOS/meta \
  --meta pr=66 \
  --meta state=classified

# 5. Enter lane and repair
cd /home/drdave/Desktop/meta/.worktrees/repair-meta-pr-66/meta
gh run view <RUN_ID> --repo FlexNetOS/meta --log
# patch, test, commit, push

# 6. Watch and merge
gh pr checks 66 --repo FlexNetOS/meta --watch
gh pr merge 66 --repo FlexNetOS/meta --auto --squash --delete-branch
```

## Immediate implementation recommendation

Build the first supervisor as a thin script or `meta git`-adjacent prototype that shells to the
current command surface. Do not require top-level `meta worktree` until the alias/parity gap is fixed.
