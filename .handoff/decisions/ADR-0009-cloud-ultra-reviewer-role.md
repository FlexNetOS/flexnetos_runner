# ADR-0009: Treat `cloud_ultra` as a symbolic reviewer role

Status: Accepted
Date: 2026-06-27
Task: `TASK-E20283-0001`

## Context

`flexnetos_runner` is becoming the GitHub operations control plane for the FlexNetOS workspace:
runner registration, runner-group repair, PR repair supervision, and merge/review policy are being
consolidated here instead of being scattered across individual repositories.

The local handoff policy currently names `cloud_ultra` as the required reviewer:

```toml
[merge]
require_review = true
reviewer       = "cloud_ultra"
permission_gate = true
```

A live GitHub review request failed because `cloud_ultra` is not a resolvable GitHub user. That
failure is useful: the policy name should not be treated as a personal account. The name is better
understood as a reviewer capability/role that may later be backed by a GitHub team, bot account, or
agent team.

## Decision

Keep `cloud_ultra` as a symbolic reviewer role and agent-team identity, not as a GitHub username.

`flexnetos_runner` owns the GitHub operations policy needed to resolve that role for this execution
plane. Any tool that requests review must resolve the symbolic role to a concrete target before
calling GitHub.

Preferred resolution order:

1. GitHub team: `FlexNetOS/cloud-ultra`.
2. Explicit checked-in fallback reviewer/team configured for the repo.
3. Fail closed with a clear unresolved-reviewer diagnostic.

Do not silently ignore an unresolved `cloud_ultra` request, and do not downgrade the policy by
removing review requirements just because the GitHub identity does not exist yet.

## Consequences

- `.handoff/policy.toml` may continue to say `reviewer = "cloud_ultra"` because that is the policy
  role, not the transport target.
- A future resolver file or command should map symbolic reviewer roles to GitHub teams/users/bots.
- The first preferred concrete target should be a GitHub team named `FlexNetOS/cloud-ultra`, allowing
  humans, bot users, or future agent team members to rotate behind the role without changing policy.
- PR tooling must report unresolved reviewer roles as policy blockers instead of best-effort warning
  noise.
- Consolidating GitHub policy here is acceptable: other repositories can still own code changes, but
  this repo owns the GitHub execution/review control-plane contracts.

## Follow-up work

- Add a checked-in reviewer resolver, for example `.handoff/reviewers.toml` or
  `.github/reviewers.toml`, with a `cloud_ultra` entry.
- Add a `reviewer resolve cloud_ultra` or equivalent PR-repair/GitHub-control command.
- Add a check that validates configured symbolic reviewers resolve before attempting
  `gh pr edit --add-reviewer`.
