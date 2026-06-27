# Agentic PR Failure Repair Queue — Brainstorm Notes

Created: 2026-06-26
Context: FlexNetOS self-hosted runners are healthy after adding meta repos to the org runner group; queued jobs are now executing and surfacing real PR failures.

## Executive summary

The best next step is **not** to manually fix every failed PR ad hoc. The target is a full agentic workflow, so failed PRs should become a structured repair queue with durable state, clear ownership, and per-PR repair agents.

The runners are now functioning as the execution plane. The missing layer is an orchestration/control loop that turns failing checks into classified repair work, assigns work to live or fresh agents, verifies fixes, and merges green PRs.

## Current operating diagnosis

- Runner health is good:
  - `fxrun-drdave-TRX50-AI-TOP-flexnetos-01`: online
  - `fxrun-drdave-TRX50-AI-TOP-flexnetos-02`: online
- The runner group access fix worked:
  - `FlexNetOS/meta` jobs that were stuck queued began running.
- The backlog is no longer primarily a runner availability issue.
- The backlog is converting into real CI outcomes:
  - Linux integration failures
  - Ubuntu test failures
  - Windows failures
  - stale queued checks from before the runner-group fix

## Recommended strategy

### 1. Refresh stale checks first

Before assigning repair work, classify and refresh failures:

- **Stale queued before runner-group access fix**: rerun checks.
- **Real failure after runner pickup**: create repair ticket.
- **Superseded PR or stale branch**: close, rebase, or regenerate instead of fixing directly.
- **Windows-only failures**: route to a separate platform lane.
- **Repeated Linux integration failures across multiple PRs**: treat as possible base/test-suite/environment failure, not necessarily PR-specific breakage.

### 2. Message the current session only when it owns the PR

Use the original/current session when all are true:

- it created the PR,
- it still owns the branch/worktree/context,
- it is alive/reachable,
- the failure appears related to its change.

Suggested message format:

```text
PR #NN failed checks: <check names>.
Pull latest CI logs, identify root cause, patch only your PR branch, run local/remote gates, push, and update the PR with diagnosis + validation.
Stop when CI is green or when the blocker is explicitly classified as outside this PR.
```

If the session is stale, confused, unreachable, or context-polluted, do **not** revive it. Spawn a clean repair agent with a structured handoff.

### 3. Use fresh repair agents for stale/stacked PRs

For a full agentic workflow, prefer:

- one **supervisor/orchestrator**,
- one **repair agent per PR**,
- one **review/merge gatekeeper**,
- self-hosted runners as execution plane,
- durable memory/ledger as source of truth.

Each repair agent should receive:

- repository and PR URL,
- branch name,
- failing checks,
- relevant CI logs,
- allowed scope/files,
- expected validation commands,
- merge target,
- commit/push/PR-update requirements,
- stop condition.

### 4. Keep the supervisor separate from repair workers

The supervisor should not manually fix five unrelated PRs in one context. It should:

1. inspect fleet/PR state,
2. classify failures,
3. open repair tickets,
4. assign agents,
5. monitor CI,
6. rerun stale checks,
7. merge green PRs,
8. store lessons learned.

That avoids context collapse and aligns with long-horizon agentic workflow design.

## Proposed control loop

```yaml
loop: pr_failure_repair
inputs:
  - open_pull_requests
  - check_runs
  - workflow_runs
  - runner_api_state
  - local_runner_journals
  - repo_policy

states:
  - discovered
  - classified
  - assigned
  - repairing
  - awaiting_ci
  - green
  - merged
  - blocked
  - closed_stale

classifications:
  stale_queued:
    action: rerun_or_cancel_and_rerun
  runner_infra:
    action: fix_runner_group_or_runner_service
  merge_conflict:
    action: rebase_or_update_branch
  test_regression:
    action: assign_repair_agent
  platform_specific:
    action: assign_platform_lane
  repeated_cross_pr_failure:
    action: create_base_failure_incident
  policy_failure:
    action: patch_metadata_or_policy
  superseded:
    action: close_or_replace

agent_assignment:
  prefer_original_session_when:
    - session_alive
    - owns_branch
    - failure_related_to_change
  otherwise:
    - spawn_clean_repair_agent

verification:
  - local_repro_if_available
  - targeted_test
  - full_required_gate
  - remote_ci_green
  - no_unrelated_diff

merge_policy:
  - commit_and_push_every_chunk
  - open_or_update_pr_immediately
  - enable_automerge_when_possible
  - merge_green_pr_directly_when_allowed
```

## Recommended priority order for current failed PR pile

1. **Runner smoke / meta CI PRs**
   - They validate the execution plane and unlock confidence in the rest.
2. **Meta/envctl infrastructure PRs**
   - They affect orchestration, runners, and future agents.
3. **PRs with repeated Linux integration failures**
   - Determine whether this is a shared base/test issue.
4. **Dependabot/release PRs**
   - Batch or close/reopen if stale.
5. **Old stale PRs**
   - Close, rebase, or regenerate. Do not let them clog the active repair queue.

## Decision matrix

| Situation | Best action |
|---|---|
| PR has queued jobs from before runner access fix | Rerun checks first |
| PR created by a live session | Message that session with exact failure payload |
| PR old/stale and session dead | Spawn fresh repair agent |
| Same failure across many PRs | Create shared incident/base-fix lane |
| Windows-only failure | Route to Windows/platform lane |
| Linux integration failure on many PRs | Investigate test environment or base branch |
| PR superseded by newer work | Close with note or replace |
| Green after rerun | Merge immediately per owner rule |

## Agent prompt template: PR repair worker

```text
You are the repair agent for <repo> PR #<number>: <title>.

Goal:
Make this PR mergeable without expanding scope.

Inputs:
- PR URL: <url>
- Branch: <branch>
- Base: <base>
- Failing checks: <checks>
- Relevant logs: <links or excerpts>

Rules:
- Inspect current branch and CI logs before editing.
- Patch only the PR's intended scope unless the failure is proven shared infra.
- Run the narrow reproducer first, then required gates.
- Commit and push every completed chunk immediately.
- Update the PR with root cause, fix, and validation.
- Stop only when CI is green, PR is superseded/closed, or blocker is explicit and evidenced.

Output:
- root cause
- files changed
- validation run
- PR/check status
- next action if not green
```

## Agent prompt template: live session nudge

```text
Your PR #<number> is failing after runner backlog started draining.
Failing checks:
- <check>: <status/conclusion>

Please pull CI logs, identify root cause, patch your branch only, run targeted local gates, push, and update the PR with root cause + validation.
If this is not caused by your PR, classify it with evidence and stop.
```

## What should not happen

- Do not manually batch-fix unrelated PRs in one context.
- Do not rerun failures blindly forever.
- Do not revive stale sessions just because they once touched the branch.
- Do not treat runner availability as the explanation once runners are online and taking jobs.
- Do not merge red PRs unless the red check is explicitly non-required and understood.
- Do not lose lessons; store recurring failure signatures.

## Reference-source alignment

### drdave-flexnetos/kclaw0

`drdave-flexnetos/kclaw0` is described as a **co-directional learning and self-update agent**. The repository structure includes memory, research, scripts, skills, tests, and bootstrap/identity files. The useful lesson here is to treat CI failures as feedback that updates the system, not as one-off interruptions.

Applied here:

- PR failures become durable repair tickets.
- Repeated failure signatures become memory.
- Agents improve routing and repair behavior over time.
- The system learns which checks are flaky, platform-specific, or high-signal.

### TDFlow-style test-driven repair

The TDFlow paper frames repository repair as a test-resolution workflow with specialized roles: propose, debug, revise, and optionally generate tests. This supports decomposing the PR repair process instead of giving one agent an enormous undifferentiated task.

Applied here:

- One agent/classifier identifies failing test/check.
- One repair agent proposes and patches.
- One verifier/gatekeeper validates and merges.

### Sherlock-style selective verification

Sherlock-style workflow reliability argues for selective verification at high-risk nodes rather than verifying every step equally. In PR repair, expensive checks should attach to the riskiest transitions:

- before merge,
- after infra changes,
- after shared test-suite changes,
- after changes touching runner/CI config.

Applied here:

- Lightweight checks for docs/config-only PRs.
- Heavy checks for runner, envctl, meta, and integration-test-affecting PRs.
- Rerun only where signal justifies cost.

## Proposed durable artifacts for future implementation

If implemented later, create something like:

```text
.handoff/pr-repair/
  queue.jsonl
  incidents/
    <timestamp>-shared-linux-integration-failure.md
  agents/
    <repo>-<pr>-assignment.md
  lessons/
    failure-signatures.yaml
```

Example queue record:

```json
{
  "repo": "FlexNetOS/meta",
  "pr": 66,
  "title": "ci: runner-smoke probe — verify self-hosted fxrun runners",
  "state": "classified",
  "classification": "mixed_stale_and_real_failure",
  "failing_checks": ["Test (windows-latest)", "Integration Tests (ubuntu-latest)"],
  "queued_checks": ["Clippy", "Integration Tests (ubuntu-latest)"],
  "runner_status": "healthy",
  "recommended_action": "rerun stale queued checks, then assign repair agent for real failures",
  "assigned_to": null,
  "last_observed": "2026-06-26T23:30:00Z"
}
```

## Final recommendation

Build/use the PR-failure control loop. For the immediate backlog:

1. Rerun stale checks affected by runner-group access.
2. Classify still-red PRs.
3. Assign live sessions only when they still own the branch.
4. Spawn clean repair agents for stale PRs.
5. Merge green PRs immediately.
6. Store every repeated failure signature as repair memory.

This is the path from “runners are working” to a true autonomous FlexNetOS PR repair and merge workflow.
