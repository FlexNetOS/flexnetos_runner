# TASK-PRR-0016 manual workflow proof on three failed PRs

Created: 2026-06-27
Task: TASK-PRR-0016

## Verdict

Manual supervisor workflow was exercised on three real failed FlexNetOS PRs. All three selected PRs were classified, lane ownership was checked, duplicate lane creation was attempted only after dry-run planning and then correctly refused by Git due existing owner worktrees, worker assignment prompts were generated against the existing residual lanes, and each PR is recorded as blocked-with-evidence pending worker repair.

## Required preflight evidence

- `rtk meta project list --json` captured 67 projects at `/tmp/task-prr-0016-projects.json`.
- `rtk meta git status` captured fleet dirty state at `/tmp/task-prr-0016-meta-status.txt`.
- `rtk meta git worktree list --json` captured 44 worktrees at `/tmp/task-prr-0016-worktrees.json` before assignment attempts.
- Current scan artifacts: `.handoff/pr-repair/scans/2026-06-27-task-prr-0016-open-prs.json`, `.handoff/pr-repair/scans/2026-06-27-task-prr-0016-classification-scan.json`, and `.handoff/pr-repair/scans/2026-06-27-task-prr-0016-selected-prs.json`.
- Live refresh artifact: `.handoff/pr-repair/scans/2026-06-27-task-prr-0016-live-refresh.json` was captured with `gh pr view <PR> --repo <repo> --json statusCheckRollup,mergeStateStatus,reviewDecision,headRefName,baseRefName,isDraft,state,updatedAt` and reconfirmed all three selected PRs are `OPEN` and `UNSTABLE` with failed checks.

## Selected PRs

### FlexNetOS/vox#3 — chore: apply handoff fleet deployment sync

- URL: https://github.com/FlexNetOS/vox/pull/3
- Head/base: `task/d633ac-handoff-fleet-sync-vox` -> `main`
- Merge state: `UNSTABLE`
- Classification: `pr_test_regression`
- Lane decision: `route_existing_owner_lane`
- Existing owner lane: `/tmp/handoff-fanout-d633ac-residual/vox`
- Lane status: `## task/d633ac-handoff-fleet-sync-vox...origin/task/d633ac-handoff-fleet-sync-vox`
- Final proof state: `blocked_with_evidence`
- Blocker: PR has failing required/CI checks and the PR head branch is already checked out in the existing residual fanout lane; duplicate meta lane creation was refused by git worktree branch ownership, so this proof routes the repair to that existing lane instead of creating a second owner.
- Assignment prompt: `.handoff/pr-repair/agents/vox-3-assignment.md`

Failed checks:
- Linux (CUDA) (CI) — https://github.com/FlexNetOS/vox/actions/runs/28289933021/job/83820099740
- Windows (CPU) (CI) — https://github.com/FlexNetOS/vox/actions/runs/28289933021/job/83820099725
- Windows (CUDA) (CI) — https://github.com/FlexNetOS/vox/actions/runs/28289933021/job/83820099730

### FlexNetOS/ruflo#3 — chore: apply handoff fleet deployment sync

- URL: https://github.com/FlexNetOS/ruflo/pull/3
- Head/base: `task/d633ac-handoff-fleet-sync-ruflo` -> `main`
- Merge state: `UNSTABLE`
- Classification: `pr_test_regression`
- Lane decision: `route_existing_owner_lane`
- Existing owner lane: `/tmp/handoff-fanout-d633ac-residual/ruflo`
- Lane status: `## task/d633ac-handoff-fleet-sync-ruflo...origin/task/d633ac-handoff-fleet-sync-ruflo`
- Final proof state: `blocked_with_evidence`
- Blocker: PR has failing required/CI checks and the PR head branch is already checked out in the existing residual fanout lane; duplicate meta lane creation was refused by git worktree branch ownership, so this proof routes the repair to that existing lane instead of creating a second owner.
- Assignment prompt: `.handoff/pr-repair/agents/ruflo-3-assignment.md`

Failed checks:
- Static regression guards (#2267 YAML + (V3 CI/CD Pipeline) — https://github.com/FlexNetOS/ruflo/actions/runs/28289931735/job/83820096108

### FlexNetOS/kasetto#2 — chore: apply handoff fleet deployment sync

- URL: https://github.com/FlexNetOS/kasetto/pull/2
- Head/base: `task/d633ac-handoff-fleet-sync-kasetto` -> `main`
- Merge state: `UNSTABLE`
- Classification: `pr_test_regression`
- Lane decision: `route_existing_owner_lane`
- Existing owner lane: `/tmp/handoff-fanout-d633ac-residual/kasetto`
- Lane status: `## task/d633ac-handoff-fleet-sync-kasetto...origin/task/d633ac-handoff-fleet-sync-kasetto`
- Final proof state: `blocked_with_evidence`
- Blocker: PR has failing required/CI checks and the PR head branch is already checked out in the existing residual fanout lane; duplicate meta lane creation was refused by git worktree branch ownership, so this proof routes the repair to that existing lane instead of creating a second owner.
- Assignment prompt: `.handoff/pr-repair/agents/kasetto-2-assignment.md`

Failed checks:
- Rust · CI (CI) — https://github.com/FlexNetOS/kasetto/actions/runs/28289929932/job/83820091811
- Next · CI (CI) — https://github.com/FlexNetOS/kasetto/actions/runs/28289929932/job/83820091809

## Lane creation evidence

Dry-run `rtk meta git worktree create ... --dry-run --json` succeeded for all three proposed repair lanes. Actual creation then failed at the child repo because Git reported the PR head branch was already used by `/tmp/handoff-fanout-d633ac-residual/<repo>`. The partial root-only repair lanes were removed with:

```bash
rtk meta git worktree remove repair-vox-pr-3 --force
rtk meta git worktree remove repair-ruflo-pr-3 --force
rtk meta git worktree remove repair-kasetto-pr-2 --force
```

## Outcome

All three PRs are intentionally recorded as `blocked_with_evidence` for this supervisor proof. The block is not lack of diagnosis; it is the correct owner-routing decision: existing residual owner lanes must be used by workers before any duplicate lane is created. A live refresh on 2026-06-27 reconfirmed the external PR state had not gone green while this proof was being finalized.

## Next action

Run the three generated worker assignments or reclassify the shared `handoff-fleet-sync` incident if the same root cause is confirmed across repositories.
