# Orchestrator / supervisor-manager identity

## Identity card

| Field | Value |
|---|---|
| id | `orchestrator-supervisor-manager` |
| role | Fleet-level controller and governance layer for runner lifecycle, registration policy, service installation, retargeting, evaluation, recovery evidence, and operational memory. |
| status | `INFERENCE`: implemented by multiple repo surfaces; `fxrun-actions` is the confirmed GitHub Actions runner supervisor binary, but the supervisor-manager role is broader than one binary. |
| scope | FlexNetOS runner fleet governance for the two local self-hosted runner slots and their durable `_work/` state. |
| owner | `TBD`: FlexNetOS runner operators. |
| primary paths | `crates/runner-actions/src/main.rs`, `scripts/install-runner-services.sh`, `scripts/retarget-local-runner-services.sh`, `scripts/eval-runners.sh`, `_work/README.md`, `_work/identity/` |
| current known labels / names | `fxrun-actions`; portable runner installer; portable user units `flexnetos-runner@01.service` / `flexnetos-runner@02.service`; legacy retarget script and legacy `actions.runner.*` units; runner evaluation script; `_work` preservation policy; supervisor-manager role. |
| last reviewed | 2026-07-02 |

## Purpose

This file records the current supervisor-manager concept from repository evidence: `crates/runner-actions/src/main.rs`, `scripts/install-runner-services.sh`, `scripts/retarget-local-runner-services.sh`, `scripts/eval-runners.sh`, and `_work/README.md`.

The supervisor-manager identity names the fleet-level authority that keeps the runner system understandable and recoverable. It is a role composed from repository code, scripts, policy, and evidence: it supervises runner lifecycle, enforces registration boundaries, installs or retargets services, collects health evidence, preserves recovery memory, and protects `_work/` as durable operational state.

## Role boundaries

- What this entity owns
  - Runner lifecycle supervision and safe runner install/register/run operations.
  - Organization/repository registration policy and exception boundaries.
  - Portable service installation, systemd adapter generation, and legacy retarget governance.
  - Runner evaluation, health evidence collection, recovery artifacts, and handoff memory.
  - Safety rails for secret hygiene, non-root execution, and `_work/` preservation.
- What this entity must not own
  - Silent host/systemd mutation without explicit confirmation.
  - Printing, committing, or copying tokens, registration secrets, private keys, auth configs, or transient credentials.
  - Inventing runner status, labels, or lane semantics not supported by evidence.
  - Deleting durable operational memory or blanket-ignoring `_work/`.
  - Converting org-scoped runners to repo-scoped runners without an explicit exception process. The supervisor-manager must not convert org-scoped runners to repo-scoped runners without that exception process.
- Upstream dependencies
  - GitHub Actions runner APIs and local runner registration state.
  - `gh`, `curl`, `systemctl`, `journalctl`, and token minting where evaluation or registration requires them.
  - Prefix-local FlexNetOS/Yazelix/Nix release paths and runner homes.
  - Operator confirmation for host/systemd or GitHub registration mutations.
- Downstream consumers
  - Runner slot 01 and slot 02 identities.
  - GitHub Actions workflows that depend on local self-hosted runners.
  - Recovery operators, future agents, and incident handoff notes.
  - `_work/evals`, `_work/archives`, and other durable evidence locations.


## Responsibility matrix

| Responsibility | Current evidence | Classification |
|---|---|---|
| Runner lifecycle supervision | `fxrun-actions` install/register/run-once/register commands | FACT |
| Org/repo registration policy | `fxrun-actions` defaults to org and names repo scope as explicit exception | FACT |
| Service installation / retargeting governance | `scripts/install-runner-services.sh` and `scripts/retarget-local-runner-services.sh` | FACT |
| Evaluation and health evidence collection | `scripts/eval-runners.sh` writes summaries, metrics, API snapshots, logs, journals, and diagnostics under `_work/evals` | FACT |
| Recovery/handoff memory | `_work/README.md`, `_work/archives`, `_work/evals`, and `_work/identity` | FACT |
| Safety rails and secret hygiene | `fxrun-actions` confirm gates and token-not-printing rule; issue #209 safety requirements | FACT |
| Preserving `_work/` durable state | `_work/README.md` preservation policy | FACT |
| Single implementation owner | Not proven; role is composed from repo surfaces | QUESTION |

## Rules

- Treat the supervisor-manager as a role unless a future repo change proves one binary fully owns it.
- Reference `fxrun-actions` specifically only for GitHub Actions runner supervisor responsibilities proven in `crates/runner-actions/src/main.rs`.
- Keep service installation prefix-derived where portable scripts support it; do not make `/etc` or hardcoded host paths the identity source of truth.
- Treat `/etc/systemd/system` system mode as a host adapter only; the portable prefix and `_work` state remain authoritative.
- Preserve `_work/` durable state and record low-volume identity/evidence metadata in Git.
- Mark missing ownership, lane priority, rotation, or failover details as `QUESTION`.

## Policy

- `POLICY`: org scope is canonical; repo scope is an explicit exception only.
- `POLICY`: host or GitHub mutations require explicit confirmation.
- `POLICY`: tokens are never printed or committed.
- `POLICY`: systemd service installation and retargeting are governance actions, not hidden side effects.
- `POLICY`: durable `_work/` topology, registration metadata, and evidence are preserved; large generated payloads are not committed.

## Constitution

1. Evidence beats assumption: runner status, paths, and labels must be inspected before being documented as facts.
2. Safety beats convenience: no secret material belongs in logs, commits, identity files, or PR bodies.
3. Portability beats host drift: the release/install prefix is the source of truth for portable runner service generation.
4. Strict upgrade beats destructive cleanup: do not delete a working migration path until parity and recovery are proven.
5. Operational memory is part of the system: preserve lessons, evidence maps, unresolved questions, and handoff notes.

## Soul

The supervisor-manager should make the runner fleet calm to operate: explicit, reversible, evidence-backed, prefix-oriented, secret-safe, and honest about unknowns.

## Lessons

- 2026-07-02: The supervisor-manager is best documented as a composed role. `fxrun-actions` owns runner supervisor operations, while shell scripts and `_work/` policy own service generation, retargeting, evaluation, and evidence preservation.
- 2026-07-02: Identity files should distinguish facts from policy and questions so future operators do not mistake useful doctrine for observed runtime state.

## Questions

- `QUESTION`: Which human or team is the durable owner for supervisor-manager policy?
- `QUESTION`: What is the formal exception process for repo-scoped runner registration?
- `QUESTION`: What exact thresholds move a runner from evaluation failure to restart, re-registration, or host repair?
- `QUESTION`: Where should incident-level identity updates be reviewed before merge?
- `QUESTION`: Which workflow is the authoritative canary for portable user-systemd runner operation?

## Recovery / handoff notes

- Use `_work/identity/` as the first orientation map for runner identity and authority boundaries.
- Use `scripts/install-runner-services.sh --dry-run` to inspect generated portable units before writing host state.
- Use `scripts/retarget-local-runner-services.sh` only as an explicitly retained legacy migration path.
- Use `scripts/eval-runners.sh` for evidence collection when live runner evaluation is appropriate and authorized.
- Recheck live user-systemd and legacy systemd unit state before service repair so duplicate listeners are not created.
- Preserve `_work/archives/*/*.sha256`, README files, and low-volume evidence; do not commit full archives, caches, or downloaded runner internals.

## Evidence map

| Marking | Claim | Evidence |
|---|---|---|
| FACT | `fxrun-actions` is a self-hosted GitHub Actions runner supervisor. | Module docs and CLI metadata in `crates/runner-actions/src/main.rs`. |
| FACT | `fxrun-actions` installs upstream runner binaries, obtains registration tokens through `gh`, registers runners, and runs jobs. | `crates/runner-actions/src/main.rs`. |
| FACT | Org scope is the default and repo scope is an explicit exception. | `crates/runner-actions/src/main.rs`. |
| FACT | Portable service generation keeps runner binaries, workspaces, homes, `.path`, auth wiring, and `RUNNER_WORKSPACE` under a prefix. | `scripts/install-runner-services.sh`. |
| FACT | Legacy retargeting exists and writes systemd services for both local slots. | `scripts/retarget-local-runner-services.sh`. |
| FACT | At last review, live portable user units existed for both runner slots. | `systemctl --user show flexnetos-runner@01.service flexnetos-runner@02.service` output captured during issue #209 correction. |
| FACT | Runner evaluation collects GitHub API snapshots, systemd state, logs, diagnostics, and metrics under `_work/evals`. | `scripts/eval-runners.sh`. |
| FACT | `_work/` is the repo-local operations root and must not be blanket-ignored. | `_work/README.md`. |
| INFERENCE | The supervisor-manager role spans code, scripts, policy, and evidence rather than one binary. | Responsibilities are split across `crates/runner-actions`, service scripts, evaluation script, and `_work/README.md`. |
| POLICY | Do not silently mutate host/systemd or GitHub registration state. | `fxrun-actions` confirmation flags; issue #209 safety requirements. |
| POLICY | Do not print or commit tokens/secrets. | `crates/runner-actions/src/main.rs` token note; issue #209 safety requirements. |
| QUESTION | Final owner and escalation path are not proven. | No inspected repo file names a durable owner. |
