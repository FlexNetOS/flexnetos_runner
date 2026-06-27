# PR Repair State Decision

Created: 2026-06-27
Parent idea: `.handoff/.idea/agentic-pr-failure-repair-queue.v5-control-plane-map.md`
Status: design decision for v1 prototype

## Decision

Use hybrid state with one canonical owner per state type. Do not create a new queue database for v1.

| State type | Canonical owner for v1 | Committed? | Reason |
|---|---|---:|---|
| Fleet graph | `.meta.yaml` via `meta project list --json` | yes | Existing workspace truth. |
| Active repair lane | `~/.meta/worktree.json` via `meta git worktree` | no | Live machine/worktree state; includes custom metadata and TTL. |
| Queue/audit projection | `.handoff/pr-repair/queue.jsonl` | yes | Human/agent-readable durable queue view; can be regenerated if ledger wins later. |
| Rich task/event ledger | handoff ledger / events export | mixed | Adopt after exact ledger schema/CLI is mapped; binary caches stay gitignored unless policy says otherwise. |
| Repeated failure signatures | ICM | no | Cross-session memory; store after confirmed repeated/root-caused signatures. |
| Local lessons view | `.handoff/pr-repair/lessons/*.md` or YAML | yes | Optional repo-visible projection of high-value lessons. |
| Code intelligence | GitKB / `.kb` | repo-dependent | Query surface, not queue state. |
| Task graph/UX | Beads if validated | TBD | Candidate UI/planning layer, not v1 source of truth. |
| Assignment runtime | Grit/Weave/agent session | no | Runtime ownership should be reflected in lane metadata, not become canonical queue state. |
| External PR truth | GitHub PR/check APIs | remote | Mergeability/checks/reviews are authoritative remotely. |

## Why not one state store yet

The V5 TODO explicitly warned not to duplicate systems before inspecting handoff, Beads, ICM, GitKB,
and Grit. Current evidence supports a staged path:

- `meta git worktree` already persists lane metadata with custom key/value fields.
- Handoff already has a ledger/task vocabulary and ADRs about committed JSONL event exports vs local
  binary caches.
- ICM is already mandatory for persistent memory and ideal for failure signatures.
- Beads may provide graph/kanban UX, but it should not block the first supervisor proof.
- GitHub must remain the remote source for check/merge state.

## Minimum v1 queue record

If `.handoff/pr-repair/queue.jsonl` is used as the committed projection, each row should be append-only
or replace-by-key through a tool, not hand-edited casually.

```json
{
  "schema": "flexnetos.pr_repair.v1",
  "id": "FlexNetOS/meta#66",
  "repo": "FlexNetOS/meta",
  "project": "meta",
  "pr": 66,
  "title": "ci: runner-smoke probe",
  "url": "https://github.com/FlexNetOS/meta/pull/66",
  "head_ref": "smoke/runner-verify",
  "base_ref": "main",
  "state": "classified",
  "classification": "pr_test_regression",
  "lane": "repair-meta-pr-66",
  "lane_command": "rtk meta git worktree create ...",
  "assigned_agent": null,
  "required_checks": [],
  "failing_checks": [],
  "last_run_ids": [],
  "next_action": "create lane and assign worker",
  "updated_at": "2026-06-27T00:00:00Z"
}
```

## State transition ownership

| Transition | Writer |
|---|---|
| discovered -> classified | supervisor |
| classified -> lane_created | supervisor after `meta git worktree create` succeeds |
| lane_created -> assigned | supervisor/assignment bridge |
| assigned -> repairing | worker after first checkpoint |
| repairing -> awaiting_ci | worker after push |
| awaiting_ci -> green/blocked | supervisor/verifier from GitHub checks |
| green -> merged | verifier |
| any -> superseded | supervisor/verifier with PR evidence |

## Text vs ledger rule

For v1, `.handoff/pr-repair/queue.jsonl` may be treated as the committed queue projection. If handoff
ledger integration is adopted later, then:

- handoff ledger/events become canonical for audit transitions,
- queue JSONL becomes a generated view,
- binary ledger caches remain local unless handoff policy explicitly changes,
- and PR repair commands must not maintain two independent sources of truth.

## Repeated failure signatures

Use ICM when a failure is confirmed and useful beyond a single PR:

```bash
icm store -t errors-resolved \
  -c "PR repair signature: <root cause>; affected checks/repos; repair playbook" \
  -i high \
  -k "pr-repair,<signature>,<repo>,<check>"
```

A committed lessons projection can be added later only for stable playbooks.

## Open follow-up inspections

- Confirm handoff's current CLI for task claim/checkpoint/done and ledger JSONL export.
- Inspect Beads enough to decide whether it owns repair graph UX.
- Inspect Grit enough to decide whether it owns agent assignment or only local code-claim locks.
- Inspect GitKB query commands for worker prompts.
