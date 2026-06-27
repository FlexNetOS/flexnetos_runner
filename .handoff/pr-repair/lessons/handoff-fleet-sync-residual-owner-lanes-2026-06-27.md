# PR repair lesson — handoff fleet sync residual owner lanes

Created: 2026-06-27
Task: TASK-PRR-0016

## Signature

Multiple `chore: apply handoff fleet deployment sync` PRs (`vox#3`, `ruflo#3`, `kasetto#2`) are failed/unstable and their PR head branches are already checked out under:

```text
/tmp/handoff-fanout-d633ac-residual/<repo>
```

Attempting to create new `meta git worktree` repair lanes correctly fails because Git refuses a branch that is already used by another worktree.

## Playbook

1. Treat the existing residual worktree as the owner lane.
2. Do not create a duplicate `repair-<repo>-pr-<n>` lane unless the residual lane is cleaned or ownership is explicitly transferred.
3. Generate a worker assignment targeting the residual lane.
4. Record the final state as `blocked_with_evidence` until a worker patches/pushes and CI is green.

## Why this matters

This proves the `.idea` owner-lane ambiguity rule in a live case: the supervisor must check branch/worktree ownership before assigning a worker, and must route to the existing owner instead of creating a second lane.
