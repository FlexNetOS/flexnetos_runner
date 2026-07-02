# Runner slot 01 identity

## Identity card

| Field | Value |
|---|---|
| id | `runner-slot-01` |
| role | Persistent FlexNetOS self-hosted GitHub Actions runner slot. |
| status | `FACT`: configured as an org-scoped FlexNetOS runner; live service status should be rechecked before incident work. |
| scope | GitHub organization runner for `https://github.com/FlexNetOS`; repo-local state under `_work/`. |
| owner | `TBD`: FlexNetOS runner operators / supervisor-manager role. |
| primary paths | `_work/repos/actions-runner-01`, `_work/actions-runner-01-work`, `_work/runner-home-01`, `_work/repos/actions-runner-01/.runner` |
| current known labels / names | `fxrun-drdave-TRX50-AI-TOP-flexnetos-01`; portable unit `flexnetos-runner@01.service`; legacy unit `actions.runner.FlexNetOS.fxrun-drdave-TRX50-AI-TOP-flexnetos-01.service`; shared labels are expected from repo policy as `self-hosted`, `linux`, `x64`, `local`, `flexnetos`. |
| last reviewed | 2026-07-02 |

## Purpose

Runner slot 01 is one of the two durable local runner lanes for the FlexNetOS organization. Its identity preserves the slot number, runner name, registration scope, work folder, home path, and service path family needed to recover or migrate the runner without relying on host memory alone.

## Role boundaries

- What this entity owns
  - The slot `01` runner installation directory `_work/repos/actions-runner-01`.
  - The slot `01` work folder `_work/actions-runner-01-work`.
  - The slot `01` service home `_work/runner-home-01`.
  - Slot-specific operational evidence and lessons appended to this identity file.
- What this entity must not own
  - Fleet-wide registration policy, systemd migration policy, or secret issuance policy.
  - Slot `02` state or recovery decisions.
  - GitHub tokens, registration secrets, private auth config, or transient session material.
- Upstream dependencies
  - GitHub organization runner registration for `FlexNetOS`.
  - The supervisor-manager role and service installation scripts.
  - Prefix-local `_work/` preservation policy.
  - Codex and GitHub auth wiring supplied through service environment variables, not through this identity file.
- Downstream consumers
  - `scripts/install-runner-services.sh` generated units and `.path` files.
  - `scripts/retarget-local-runner-services.sh` legacy migration path while retained.
  - `scripts/eval-runners.sh` and runner-smoke workflows.
  - Future operators and agents doing recovery, evaluation, or handoff.

## Rules

- Keep slot `01` paths under the install prefix unless a documented migration proves a new prefix.
- Treat the whitelisted `.runner` fields as recovery metadata, not as a source for secrets.
- Do not copy `.runner` `serverUrl` or broker endpoint fields into this file.
- Do not edit `.runner`, generated runner internals, auth files, or service files from this identity document.
- Append dated lessons after incidents; do not rewrite history.

## Policy

- `POLICY`: `_work/` is important repo-local operations state and must not be blanket-ignored.
- `POLICY`: runner identity records are durable, low-volume operational state and should be tracked.
- `POLICY`: this slot should remain org-scoped unless an explicit exception process approves repo-scoped registration.
- `POLICY`: no secrets, tokens, private keys, session material, or transient credentials belong in identity files.

## Constitution

1. Slot identity is stable even when host services are retargeted or migrated.
2. Registration claims must be backed by evidence or marked `QUESTION`.
3. The runner service must execute as non-root or an equivalent constrained runner user.
4. Durable `_work/` metadata is preserved; heavyweight caches, archives, generated internals, and transient logs are not identity content.

## Soul

Slot 01 should be boring, recoverable, and evidence-driven: preserve enough identity to restore service quickly, but never enough sensitive material to compromise the runner fleet.

## Lessons

- 2026-07-02: Preserve runner slot identity separately from host-specific systemd units so service retargeting or portable user-systemd migration does not erase the operator's understanding of the slot.

## Questions

- `QUESTION`: Is slot 01 the primary continuity lane, or are both slots intentionally symmetric?
- `QUESTION`: What is the exact failover order when slot 01 and slot 02 are both online?
- `QUESTION`: Who owns label changes for this slot?
- `QUESTION`: What rotation policy applies to this runner's registration?
- `QUESTION`: Which recovery triggers require appending evidence to this file?
- `QUESTION`: What evaluation thresholds require service restart, runner re-registration, or host investigation?
- `QUESTION`: What restart rules apply when a job is active but the listener is unhealthy?

## Recovery / handoff notes

- Start with the prefix-local paths in the identity card.
- Read only safe `.runner` fields needed for identity: `agentName`, `gitHubUrl`, `workFolder`, `poolName`, and `agentId`.
- Recreate `.path` from release/Yazelix/Nix inputs instead of shell history.
- Prefer the portable user-systemd unit generated by `scripts/install-runner-services.sh`; use legacy retarget only as an explicitly retained migration path.
- After an incident, append the date, symptom, action taken, and evidence location to `Lessons` or the evidence map.

## Evidence map

| Marking | Claim | Evidence |
|---|---|---|
| FACT | Slot id is `01`. | Issue #209 mission and path family; `_work/repos/actions-runner-01`. |
| FACT | Current runner config path is `_work/repos/actions-runner-01/.runner`. | Issue #209 mission; whitelisted local config inspection. |
| FACT | Runner name is `fxrun-drdave-TRX50-AI-TOP-flexnetos-01`, agent id is `4730`, and pool is `Default`. | Whitelisted `.runner` fields `agentName`, `agentId`, and `poolName`. |
| FACT | GitHub registration scope is the FlexNetOS organization. | Whitelisted `.runner` field `gitHubUrl=https://github.com/FlexNetOS`. |
| FACT | Work folder is `_work/actions-runner-01-work` in the repo-local path family. | Whitelisted `.runner` `workFolder` and issue #209 path family. |
| FACT | Service home path family is `_work/runner-home-01`. | Issue #209 and `scripts/install-runner-services.sh`. |
| FACT | Portable units set `HOME`, `GIT_CONFIG_GLOBAL`, `CODEX_HOME`, `GH_CONFIG_DIR`, and `RUNNER_WORKSPACE`. | `scripts/install-runner-services.sh`. |
| INFERENCE | Slot 01 is one lane of a two-runner local fleet, not the whole fleet. | Issue #209 lists slots 01 and 02; scripts operate over both slots. |
| POLICY | Do not include secrets or copied endpoint fields. | Issue #209 safety rules and this schema. |
| QUESTION | Primary/canary/failover lane semantics are not proven. | No inspected repo evidence assigns slot 01 a unique lane role. |
