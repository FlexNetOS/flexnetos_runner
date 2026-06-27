# PR repair worker assignment — FlexNetOS/ruflo#3

Created: 2026-06-27
Task: TASK-PRR-0016
State: assigned-to-existing-owner-lane

## Goal

Repair FlexNetOS/ruflo#3: chore: apply handoff fleet deployment sync

PR: https://github.com/FlexNetOS/ruflo/pull/3
Branch: `task/d633ac-handoff-fleet-sync-ruflo` -> `main`
Current merge state: `UNSTABLE`
Classification: `pr_test_regression`

## Existing owner lane

Do **not** create a duplicate lane. Git worktree ownership already maps this PR branch to:

```text
/tmp/handoff-fanout-d633ac-residual/ruflo
```

Verified lane status:

```text
## task/d633ac-handoff-fleet-sync-ruflo...origin/task/d633ac-handoff-fleet-sync-ruflo
```

## Failing checks

- Static regression guards (#2267 YAML + (V3 CI/CD Pipeline) — https://github.com/FlexNetOS/ruflo/actions/runs/28289931735/job/83820096108

## Reliable CLI fallback

```bash
cd /home/drdave/Desktop/meta
rtk meta project list --json
rtk meta git status

gh pr view 3 --repo FlexNetOS/ruflo --json statusCheckRollup,mergeStateStatus,reviewDecision,headRefName,baseRefName
cd /tmp/handoff-fanout-d633ac-residual/ruflo
git status --short --branch
```

Use MCP tools only if exposed in this session. If a documented slash command or skill is unavailable, fall back to the CLI commands above.

## Rules

- Work only inside `/tmp/handoff-fanout-d633ac-residual/ruflo`.
- Patch only `FlexNetOS/ruflo#3` scope unless a supervisor reclassifies as shared incident.
- Inspect CI logs before editing.
- Run the narrow reproducer first, then required gates.
- Commit and push every completed chunk immediately.
- Update the PR with root cause, fix, and validation.
- Stop only when required CI is green and PR is mergeable/automerge armed, PR is superseded/closed, or a blocker is evidenced with exact next action.

## Stop condition for this assignment

Return a short report with root cause, files changed, validation, PR/check status, and next action if not green.
