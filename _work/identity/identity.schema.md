# Runner fleet identity schema

## Identity card

Every identity file must begin with an `Identity card` table containing these fields:

| Field | Rule |
|---|---|
| id | Stable repository-local identifier. Use `runner-slot-<slot>` for runner slots and a descriptive role id for non-slot identities. |
| slot | Required for runner identities; use `01`, `02`, etc. Non-slot role identities may omit this field or mark it `not applicable`. |
| role | One sentence naming what the entity is allowed to do. |
| status | Current operational state if proven; otherwise `UNKNOWN` or `TBD`. |
| scope | Operational scope, such as organization runner, repo-local service path, or fleet governance role. |
| owner | Account, team, or role responsible for maintenance. Use `TBD` when no owner is proven. |
| primary paths | Repository-relative paths whenever possible; absolute paths may appear only when they are direct evidence from current runner config. |
| current runner config path | Required for runner identities; must point to the slot `.runner` file without copying unsafe fields. |
| current known labels / names | Runner names, labels, service names, or role aliases proven by evidence. |
| last reviewed | ISO date of the last evidence review. |

## Purpose

The durable reason the identity exists. This section should explain why the entity matters for runner recovery, operations, or governance.

## Role boundaries

This section must include:

- What this entity owns
- What this entity must not own
- Upstream dependencies
- Downstream consumers

## Rules

Operational rules that future operators and agents must follow when changing or using the identity.

## Policy

Durable policy statements. Each policy claim must be marked `POLICY` in the evidence map or explicitly linked to a repository source.

## Constitution

Non-negotiable invariants for the identity. These should be stable rules that protect the runner fleet from drift, data loss, or unsafe mutation.

## Soul

Operating ethos for the entity. This is engineering intent and behavior, not mysticism or unverifiable claims.

## Lessons

Append dated lessons as operations produce new evidence. Do not erase old lessons; supersede them with a newer dated entry when needed.

## Questions

Open facts, decisions, or policy boundaries that lack evidence. Use `TBD`, `UNKNOWN`, or `QUESTION` instead of inventing certainty.

## Recovery / handoff notes

Practical notes for restoring, moving, rotating, or safely handing off the identity during incidents or migrations.

## Evidence map

Each operational claim must appear in an evidence map row or be marked as an open question. Allowed markings:

| Marking | Meaning |
|---|---|
| FACT | Directly observed from repository files, whitelisted `.runner` fields, live verification output, or committed policy text. |
| INFERENCE | Reasonable conclusion from facts; must state the source facts. |
| POLICY | Required operating rule from this repo, issue, or documented project policy. |
| QUESTION | Unresolved or unproven claim that must not be treated as fact. |

Evidence rows should use this shape:

| Marking | Claim | Evidence |
|---|---|---|
| FACT | Example claim. | `path/to/file`, whitelisted command output, or issue requirement. |

Secret hygiene rule: identity files must never include tokens, registration secrets, private keys, session material, private auth config, broker endpoints, or transient credentials. Do not copy `.runner` `serverUrl` or broker endpoint fields into identity files unless a later review proves they are safe and necessary. Runner identity files may record safe `.runner` fields such as `agentId`, `agentName`, `poolId`, `poolName`, `gitHubUrl`, and `workFolder`.

## Operational question requirements

Runner identity files must explicitly carry open questions for failover order, label ownership, registration rotation policy, recovery triggers, evaluation thresholds, service restart rules, and the evidence bundle that should be appended after incidents.
