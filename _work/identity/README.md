# Runner fleet identities

These files are durable, low-volume operational identity records for the FlexNetOS self-hosted runner fleet. They live under `_work/identity/` because `_work/README.md` defines `_work/` as the repo-local operations root and says runner topology, registration metadata, evaluation evidence, recovery evidence, and operational memory should be tracked when they are durable and low-volume.

## Identity files

- [Runner slot 01](runner-01.identity.md)
- [Runner slot 02](runner-02.identity.md)
- [Orchestrator / supervisor-manager](orchestrator-supervisor-manager.identity.md)
- [Identity schema](identity.schema.md)

## Why these files exist

The runner fleet has runtime configuration (`.runner`), service adapters, work folders, auth wiring, and recovery/evaluation evidence spread across several paths. These identity files collect only the safe, durable identity facts and policy boundaries needed for future recovery. They are not runtime configuration and must not become a secret store.

## Update protocol

1. Keep the identity card current when a runner name, service path, work folder, owner, label set, or status changes.
2. Append dated lessons; do not overwrite or delete old lessons without preserving their history.
3. Keep evidence maps current. Every operational claim should be marked as `FACT`, `INFERENCE`, `POLICY`, or `QUESTION`.
4. Move unresolved facts to `Questions` instead of presenting guesses as facts.
5. Never include tokens, registration secrets, private keys, session material, private auth config, broker endpoints, or transient credentials.
6. Do not edit `.runner`, auth files, service units, or generated runner internals as part of identity maintenance unless a separate, explicit operational task requires it.
7. After incidents, append dated lessons and add evidence-map rows pointing to durable evidence: `_work/evals` summaries, systemd state captures, runner diagnostic tails, workflow run URLs, or recovery notes.

## Claim markings

| Marking | Definition | How to use it |
|---|---|---|
| FACT | Direct evidence from repository files, whitelisted runner config fields, live verification, or committed policy text. | Use for names, slots, paths, service names, labels observed in tracked eval snapshots, and repo policy that were inspected. |
| INFERENCE | A conclusion drawn from one or more facts. | Use when the repo implies a role or boundary but does not name every detail directly. |
| POLICY | A durable rule that operators and agents must follow. | Use for safety rails, update rules, and issue acceptance requirements. |
| QUESTION | Unknown or unproven information. | Use for lane semantics, failover order, ownership, rotation, evaluation thresholds, and restart rules when evidence is missing. |

## Issue 209 coverage checklist

| Requirement | Identity location |
|---|---|
| Runner slot 01 identity using current repo evidence | [runner-01.identity.md](runner-01.identity.md) |
| Runner slot 02 identity using current repo evidence | [runner-02.identity.md](runner-02.identity.md) |
| Supervisor-manager role from repo evidence | [orchestrator-supervisor-manager.identity.md](orchestrator-supervisor-manager.identity.md) |
| Identity contract and required sections | [identity.schema.md](identity.schema.md) |
| Fact / inference / policy / question distinction | This README and each file's evidence map |
| Explicit unresolved operational questions | Each identity file's `Questions` section |
| Secret and mutable-runtime exclusion | This README, [identity.schema.md](identity.schema.md), and each file's `Rules` / `Policy` sections |

## Maintenance scope

These identity files are documentation and operational memory. They are not runtime configuration, not credentials, and not runner registration material. If a recovery action discovers new facts, append the evidence and lesson here after the action succeeds.
