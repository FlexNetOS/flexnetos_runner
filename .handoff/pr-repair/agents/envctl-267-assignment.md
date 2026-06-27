# PR Repair Worker Assignment — FlexNetOS/envctl#267

Generated: 2026-06-27
Lane: `repair-envctl-pr-267`
PR: https://github.com/FlexNetOS/envctl/pull/267
Repo: `FlexNetOS/envctl`
Branch: `meta-local-health-continue`
Base: `master`

## Classification

`unknown_or_incomplete_checks`

Reason: manual supervisor proof selected this PR because its branch was not already checked out in a
repair/planning lane. The PR is open and non-draft, but the visible check set is sparse and green
(CodeQL-only in the captured `statusCheckRollup`) while `mergeStateStatus` is `UNKNOWN`. This is not
yet evidence of a PR test regression.

## Reliable startup

```bash
cd /home/drdave/Desktop/meta
rtk meta project list --json
rtk meta git status
rtk meta git worktree status repair-envctl-pr-267
cd /home/drdave/Desktop/meta/.worktrees/repair-envctl-pr-267/envctl
```

## First actions

1. Inspect current PR state and full check history:

   ```bash
   gh pr view 267 --repo FlexNetOS/envctl \
     --json number,title,url,state,isDraft,mergeStateStatus,headRefName,baseRefName,statusCheckRollup,reviewDecision
   gh pr checks 267 --repo FlexNetOS/envctl
   gh run list --repo FlexNetOS/envctl --branch meta-local-health-continue --limit 10
   ```

2. If checks are stale/incomplete, rerun the relevant workflow once before editing.
3. If full CI is green and mergeable, hand to verifier for automerge.
4. If a real failure appears, patch only this PR branch and scope.

## Constraints

- Work only inside `/home/drdave/Desktop/meta/.worktrees/repair-envctl-pr-267/envctl`.
- Do not mutate unrelated repos unless the supervisor reclassifies this as a shared incident.
- Commit, push, and update the PR for every completed chunk.
- Store repeated failure signatures in ICM.
- Stop at green/merged, superseded/closed, or an evidenced blocker.
