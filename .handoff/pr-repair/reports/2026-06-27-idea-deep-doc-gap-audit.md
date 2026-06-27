# Deep `.idea` PR-repair gap audit

Created: 2026-06-27
Branch: `chore/idea-pr-repair-gap-audit`
Scope: `.handoff/.idea/*.md` re-read and cross-reference against `.handoff/tasks/TASK-PRR-*.task.json` plus current implementation evidence.

## Verdict

The prior `.idea` work was **not** the intended outcome. It completed a brainstorm/design/proof checklist, but the intended outcome is still a working agentic PR-failure repair control loop.

The strongest source evidence is explicit:

- `.handoff/.idea/agentic-pr-failure-repair-queue.md:8-10` says the target is a structured repair queue/control loop that classifies failing checks, assigns repair agents, verifies fixes, and merges green PRs.
- `.handoff/.idea/pr-repair-v5-todo-completion-audit.md:15-18` says V5 TODO completion is only artifacts/design/proof, while future external PR repair automation is not implemented.
- `.handoff/.idea/pr-repair-v5-todo-completion-audit.md:61-62` says no external failing PR was patched and merged.
- `.handoff/.idea/pr-repair-v5-todo-completion-audit.md:103-107` names the next-stage implementation steps: select a failing external PR, run a worker, patch if needed, push/watch/merge, and convert the manual flow into `meta pr-repair`.
- `.handoff/.idea/pr-repair-manual-supervisor-proof.md:71-73` says the manual proof did not prove a full repair cycle through patch/push/CI-watch/merge/ICM; that requires executing the worker assignment.

## Cross-reference summary

Current `.handoff/tasks` does have broad PRR coverage:

- `TASK-PRR-0001` through `TASK-PRR-0016` plus `TASK-PRR-9999` exist and are all backlog.
- `TASK-PRR-9999` correctly names the capstone intended outcome: durable queue/state, scan/classify/lane/assign/watch/merge surface, live-session routing, platform lanes, verifier gatekeeper, GitHub App/runner integration, durable learning, and multiple real PR outcomes.
- Source-gap coverage now references all 11 `.idea` files somewhere across tasks.

But the deep re-read found **three missed work items** and **two sequencing risks**.

## Missed items now added as tasks

### MISS-001 — `loop_lib` repair-runner model was in `.idea` but had no task

Evidence:

- `.handoff/.idea/agentic-pr-failure-repair-queue.v4-research-plan.md:254-293` defines Research Track D: decide whether `loop_lib` can provide the supervisor loop/execution engine and compare state machine, retries, step budgets, watchdogs, parallel lanes, event logging, and failure classification.
- `rg` over `.handoff/tasks` showed no task objective for `loop_lib`; only `.idea` mentions it.

Added task:

- `TASK-PRR-0017` — close `loop_lib` repair-runner model inspection.

Why it matters:

Without this, the prototype could pick `meta`/handoff scripting by habit and miss an already-built loop engine for bounded repair workflows.

### MISS-002 — GitHub issue/comment ticket projection decision was open but untasked

Evidence:

- `.handoff/.idea/agentic-pr-failure-repair-queue.v3-meta-control-plane.md:482-490` asks whether repair tickets should be GitHub issues/comments, local ledger rows, or both.
- Existing tasks cover local queue projection and GitHub App/secrets, but no task forces a ticket-projection decision for GitHub issues/PR comments.

Added task:

- `TASK-PRR-0018` — decide GitHub issue/comment ticket projection boundaries.

Why it matters:

If this remains implicit, agents may split repair state across local JSONL, PR comments, GitHub issues, and handoff tasks without a single projection rule.

### MISS-003 — ICM storage/query model was only partially captured

Evidence:

- `.handoff/.idea/agentic-pr-failure-repair-queue.v4-research-plan.md:375-417` asks for actual ICM storage/query model: data location, DB engine/schema, topics/keys/links/importance, recall implementation, failure-signature retrievability, and anti-noise policy.
- `TASK-PRR-0012` covers durable failure-signature learning, but it does not explicitly require inspecting the ICM storage/query model or proving recall behavior against the intended signature schema.

Added task:

- `TASK-PRR-0019` — validate ICM storage/query model for PR-repair failure signatures.

Why it matters:

Storing signatures is not enough. The supervisor has to recall and de-duplicate them reliably before opening duplicate lanes.

## Sequencing risks not fixed by task creation

### RISK-001 — Task graph allows prototype work before the `.idea` immediate-order proof

Evidence:

- `.handoff/.idea/agentic-pr-failure-repair-queue.v3-meta-control-plane.md:407-429` says immediate order is: manual supervisor mode on 2-3 failed PRs, then thin wrapper, then assignment, then verifier/merge automation, then learning.
- `TASK-PRR-0002` (thin `meta pr-repair` prototype) is blocked only by `TASK-PRR-0007`, not by `TASK-PRR-0016` (2-3 failed PR manual proof).

Consequence:

The task graph can still lead agents into building the wrapper before proving the 2-3 live PR workflow that `.idea` says should come first.

Recommended follow-up:

After the handoff kernel supports safe retargeting/relocking, either update `TASK-PRR-0002.blocked_by` to include `TASK-PRR-0016`, or add a supervisor rule that `TASK-PRR-0016` is the next P0 proof before prototype implementation.

### RISK-002 — Existing capstone still names only `TASK-PRR-0001` through `TASK-PRR-0016`

Evidence:

- `TASK-PRR-9999` currently says all prerequisite `TASK-PRR-0001` through `TASK-PRR-0016` must be done, so new tasks `0017-0019` are not in its textual acceptance gate.

Consequence:

A future verifier could mark the capstone complete without the three newly surfaced missed items unless it reads this deep audit or the task list directly.

Recommended follow-up:

Relock or supersede the capstone with a kernel-supported update that includes `TASK-PRR-0017` through `TASK-PRR-0019` without breaking intent-lock discipline.

## Current implementation gaps still correctly represented by existing tasks

These were not newly missed, but they are still unimplemented:

1. No `meta pr-repair` or thin script wrapper exists yet. Local search of `meta_cli`, `meta_core`, `.meta/plugins`, and `scripts` found no `pr-repair` implementation.
2. No canonical `.handoff/pr-repair/queue.jsonl` projection exists yet; only dated historical queue artifacts exist.
3. No end-to-end external PR repair has been proven; current proof was repair-system self-work, not a real external PR patch/merge.
4. No implemented Claude/Codex parity verifier or `meta init codex` exists.
5. No verifier/merge gatekeeper component exists.
6. No platform lane / required-vs-advisory check map exists.
7. No live-session ownership/nudge mechanism exists.
8. No runner/GitHub App trigger/secrets decision exists.

These map to existing tasks `TASK-PRR-0002` through `TASK-PRR-0016` and capstone `TASK-PRR-9999`.

## Bottom line

What we missed was not another Codex hook/config surface. We missed that `.idea` is about a **meta-native PR repair supervisor/control loop**, and that the completed V5 work only proved design/publishing mechanics.

The gap is now clearer:

```text
Current state: design artifacts + task backlog + one manual lane/assignment proof.
Intended outcome: working PR repair control loop with real external PR outcomes.
Newly surfaced missing tasks: loop_lib fit, GitHub issue/comment projection, ICM storage/query proof.
Primary next action: run `TASK-PRR-0016` manual workflow on 2-3 failed PRs before building the wrapper.
```
