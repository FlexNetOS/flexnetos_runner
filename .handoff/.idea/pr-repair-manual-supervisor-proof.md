# PR Repair Manual Supervisor Proof

Created: 2026-06-27
Parent idea: `.handoff/.idea/agentic-pr-failure-repair-queue.v5-control-plane-map.md`
Status: first manual proof of V5 section B

## What was proven

The manual supervisor flow can already run against live FlexNetOS PR state with current tooling:

1. Fleet preflight ran:
   - `rtk meta project list --json`
   - `rtk meta git status`
   - `rtk meta git worktree list --json`
2. Open PR scan ran through GitHub search/API.
3. Candidate PRs were classified from `gh pr view ... statusCheckRollup`.
4. Existing branch/worktree ownership was checked before assigning a lane.
5. A collision was detected for `FlexNetOS/weave#157` because its branch was already checked out in
   `plan-weave-red/weave`.
6. The partial failed lane was removed with `rtk meta git worktree remove repair-weave-pr-157 --force`.
7. A non-colliding lane was created for `FlexNetOS/envctl#267`:
   - lane: `repair-envctl-pr-267`
   - path: `/home/drdave/Desktop/meta/.worktrees/repair-envctl-pr-267`
   - repos: `.` and `envctl`
   - metadata: `kind=pr-repair`, `repo=FlexNetOS/envctl`, `pr=267`, `source=manual-supervisor-proof`
8. A worker assignment prompt was generated at:
   - `.handoff/pr-repair/agents/envctl-267-assignment.md`

## Important correction discovered

The first attempted lane used `FlexNetOS/weave#157`, which was classified as a real regression, but
its branch already had an existing lane. This is not a failure of the model; it proves the supervisor
must check current worktree ownership before assigning a worker.

The second lane used `FlexNetOS/envctl#267` because it had no branch collision. After inspection, its
proper classification is `unknown_or_incomplete_checks`, not `pr_test_regression`: the visible checks
were sparse/green while mergeability was `UNKNOWN`. The generated worker prompt therefore starts with
check-history inspection and possible rerun, not code editing.

## Sample live classifications

Captured classifications are stored in:

```text
.handoff/pr-repair/scans/2026-06-27-classifications.json
```

Examples observed:

| PR | Classification | Notes |
|---|---|---|
| `FlexNetOS/weave#159` | `awaiting_ci` | Mostly green, one in-progress check. |
| `FlexNetOS/envctl#285` | `green_candidate` | Green checks but branch behind. |
| `FlexNetOS/meta#69` | `awaiting_ci` | Mixed failures plus in-progress integration check. |
| `FlexNetOS/envctl#284` | `pr_test_regression` | `gates` failed. |
| `FlexNetOS/grit#5` | `pr_test_regression` | Linux/macOS failures. |
| `FlexNetOS/meta#68` | `merge_conflict` | Dirty merge state plus failures. |
| `FlexNetOS/envctl#280` | `pr_test_regression` | Clippy failed. |

## TODO status covered

This proof covers these V5 section B items:

- run a manual scan of open FlexNetOS PRs and check states;
- classify at least 2-3 PRs;
- rerun stale checks before assigning workers, with the caveat that no selected lane had a confirmed
  stale queued state requiring rerun;
- create one repair lane manually with existing `meta git worktree` commands;
- generate one worker prompt from the v3/v5 template.

It does not yet prove a full repair cycle through code patch, push, CI watch, merge/automerge, or ICM
failure-signature storage. That requires selecting an actual repair target and executing the worker
assignment.
