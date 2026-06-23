# flexnetos_runner Automation + User Story

This document is the post-audit operating map for `flexnetos_runner`: every major component, every
flow of data, which steps are automated today, which still require a user/operator, and the backlog
items that close the gaps found in the 2026-06-23 deep code audit.

## Component inventory

| Component | Owner | Runs where | Automated today | Not automated / pending |
|---|---|---|---|---|
| `flexnetos_github_app` | Control plane | GitHub/App host | Builds signed dispatch frames and sends jobs to the local runner. | Must provide signed provenance/status freshness seams for some backlog items. |
| `fxrun-dispatch` | Execution-plane dispatcher | Local machine | UDS server, HMAC verification, admission gates, kernel routing, subprocess bounds, audit/recovery. | Socket hardening, full-envelope signing, concurrent serve/global cap, structured result status. |
| `runner-core` | Pure policy core | Library | JobSpec, wire frames, routing, gates, events, recovery, cost, risk, workspace contracts. | Some new pure policies from backlog: envelope auth, rule citations, lifecycle FSM, result contracts. |
| `runner-actions` / `fxrun-actions` | Actions runner supervisor | Local machine | Installs/registers/runs GitHub self-hosted runner, enforces minimum version. | Runner tarball verification; avoid token-in-argv exposure. |
| `fxrun` | Operator CLI | Local shell / desktop terminal | `route`, `agents`, `doctor` surfaces routing and seams. | More user-facing health/approval/event inspection commands. |
| `envctl` / secrets | Secret plane | Local machine | Transitional env injection via `FXRUN_DISPATCH_KEY`, `FXRUN_INJECT_SECRETS`. | Vault-native key retrieval remains the target; no raw env as final resting place. |
| `loop` / `atc` / `hf` / `weave` | Existing kernels | Child processes | Runner delegates and bounds execution; kernels own work semantics. | Structured result/status contract, fan-out accounting, output coverage, rollback/checkpoint signals. |
| Audit logs | Observability plane | Local files | NDJSON all-events + policy-only streams, redaction, recovery directives, risk/cost annotations. | Rule citation schema and queryable lifecycle story. |
| Desktop app | User surface | Local desktop | Conceptual front door for status/approval/re-arm. | Not implemented here; story below defines needed entry/exit points. |

## End-to-end data and control flow

```text
Legend: [A] automated now   [P] planned automation   [U] user/operator decision

GitHub event / user request
        |
        v
+-----------------------+        [A] derive repo/pr/job facts
| flexnetos_github_app  |        [P] derive signed submitter + freshness/check status
+-----------------------+
        |
        | DispatchRequest over local UDS
        | - spec_json + HMAC(signature)              [A]
        | - approval grant                           [A when needed]
        | - submitter provenance                     [P: must be signed]
        | - deadline / idle timeout                  [A]
        v
+-----------------------+        [A] verify frame, lint, scan, fork isolation
| fxrun-dispatch        |        [A] authority/approval/target/state/rate/breaker/budget gates
| admission pipeline    |        [A] recovery directive on every rejection
+-----------------------+
        |
        | KernelPlan(route_id, kernel, repo, agent, intent)
        v
+-----------------------+        [A] acquire workspace, inject secrets, spawn child
| SubprocessInvoker     |        [A] deadline/idle kill, cost relay, stderr capture
+-----------------------+
        |
        | stdin: JobSpec
        | env: FXRUN_JOB_ID, FXRUN_REPO, FXRUN_AGENT, FXRUN_COST_FILE, injected secrets
        v
+-------------------------------+
| Existing kernel child process |
| loop / atc / hf / weave       |
+-------------------------------+
        |
        | exit status + stderr + optional cost JSON  [A]
        | structured result/status                   [P]
        v
+-----------------------+        [A] classify fatal/transient/timeout/idle
| fxrun-dispatch return |        [A] update retry/quarantine/risk/budget/rate ledgers
+-----------------------+
        |
        +--> DispatchResponse to App/weave           [A]
        |    accepted or rejected + recovery advice
        |
        +--> FXRUN_EVENT_LOG / FXRUN_POLICY_LOG      [A]
             redacted NDJSON audit trail
```

## Admission pipeline detail

```text
raw UDS bytes
  |
  v
[constitution seal] --changed--> reject all                      [A]
  |
  v
[parse DispatchRequest] --bad JSON--> reject                     [A]
  |
  v
[verify spec_json HMAC] --bad sig--> reject                      [A]
  |
  v
[authority floor] --low/missing submitter--> reject              [A/P: submitter must be signed]
  |
  v
[structural lint] --bad repo/blank ids--> reject                 [A]
  |
  v
[content scan] --injection pattern at threshold--> reject        [A]
  |
  v
[fork isolation] --fork PR--> hosted-only reject                 [A]
  |
  v
[approval grant] --needed but absent/invalid--> wait.human       [A + U]
  |
  v
[quarantine] --fingerprint terminal--> reject                   [A]
  |
  v
[route + target allowlist] --kernel disallowed--> reject         [A]
  |
  v
[state gate] --survival tier too degraded--> retry-later        [A]
  |
  v
[single-flight] --same target busy--> wait/escalate             [A]
  |
  v
[rate/cooldown] --busy/cooling--> retry-after                   [A/P: clock sampling fix]
  |
  v
[loop breaker] --same work loop--> escalate                     [A]
  |
  v
[budget governor] --spent--> halt/re-arm needed                 [A + U]
  |
  v
delegate to kernel subprocess                                   [A]
```

## Automation boundary map

```text
AUTOMATED NOW
  - App -> runner dispatch frame for normal work.
  - HMAC verification of JobSpec bytes.
  - Static admission gates and recovery directives.
  - Kernel spawning when FXRUN_KERNEL_EXEC=1.
  - Deadline and idle-timeout enforcement.
  - Cost relay, risk scoring, retry/quarantine/budget/rate ledgers.
  - Redacted audit and policy logs.
  - CLI diagnostics via fxrun doctor/route/agents.

USER / OPERATOR INVOLVEMENT TODAY
  - Configure env/secrets/socket/log paths and kernel command overrides.
  - Approve jobs when FXRUN_APPROVAL_BANDS requires a grant.
  - Re-arm after budget exhaustion, quarantine, or constitution violation.
  - Install/register Actions runner with --confirm=true.
  - Interpret logs/doctor output and decide when to widen automation.

PLANNED AUTOMATION
  - Signed full-envelope provenance and status/freshness facts.
  - UDS private runtime directory and permission hardening.
  - Fresh non-adoptable workspace acquisition by construction.
  - Supply-chain verification for Actions runner artifacts.
  - Structured kernel result/status contract and output coverage gates.
  - Desktop app event stream, approval/re-arm buttons, and guided recovery.
```

## Fresh audit backlog

Each task below is intended to become one small implementation cycle: spec/test first, then patch,
verify, PR, and update this document + `docs/kclaw0-upgrade-ledger.md`.

### Tier 0 — security/correctness fixes from the audit

1. **Signed full-envelope authority provenance**
   - Gap: `DispatchRequest.submitter` is mutable outside the `spec_json` HMAC.
   - Upgrade: add an envelope MAC or per-submittee proof that binds `spec_json`, `signature`,
     `submitter`, approval/deadline/idle metadata that must be trusted, and schema version.
   - Acceptance: replaying a valid `spec_json` with a forged `owner` submitter fails before authority
     policy; legacy frames remain allowed only when the authority gate is disabled.

2. **UDS socket ownership and permission hardening**
   - Gap: server removes `socket_path` and binds without proving it is a safe socket path.
   - Upgrade: require safe parent directory ownership/mode, remove only existing sockets, set socket
     permissions to owner-only, and reject symlink/file collisions.
   - Acceptance: tests cover unsafe parent, non-socket collision, stale socket cleanup, and chmod.

3. **Fresh workspace acquisition by construction**
   - Gap: `/tmp/fxrun-ws-$PID-$jobid` + `create_dir_all` can adopt residue/precreated dirs.
   - Upgrade: unique `create_dir` or `tempfile`-style nonce; fail if a target exists; record workspace
     id in the audit; keep zero-residue teardown.
   - Acceptance: precreated path is refused or bypassed; two same-job acquisitions never share a path.

4. **Rate-limit clock freshness**
   - Gap: `now_secs` is sampled before blocking `accept()`, so the first request after idle can use
     stale time.
   - Upgrade: sample monotonic time after accept/read, immediately before `handle_request()`.
   - Acceptance: e2e/unit test proves cooldown expires while server is idle before next connection.

5. **Actions runner artifact verification**
   - Gap: `fxrun-actions install` downloads and extracts the runner archive without checksum or
     attestation verification.
   - Upgrade: verify GitHub-published SHA256 and/or artifact attestation before extract.
   - Acceptance: bad digest refuses before tar extraction; latest-version path verifies automatically.

6. **Actions registration token non-argv path**
   - Gap: token is passed as `config.sh --token <token>`, briefly visible in process argv.
   - Upgrade: prefer runner-supported stdin/env/token-file if available; otherwise isolate and document
     a fallback with process visibility warning and shortest possible lifetime.
   - Acceptance: normal path avoids token in command-line args; fallback is explicit and audited.

7. **CI supply-chain gate**
   - Gap: local `cargo audit` passes, but CI does not enforce audit/deny.
   - Upgrade: add CI job for `cargo audit` and, if policy is desired, `cargo deny` licenses/bans.
   - Acceptance: PR checks fail on known vulnerable advisories or denied duplicate/banned crates.

8. **Ledger/docs drift guard**
   - Gap: the ledger still marked state-gated route admission queued after PR #31.
   - Upgrade: add a docs consistency task/checklist requiring Applied/Backlog updates with every cycle.
   - Acceptance: PR template/check or test catches stale queued items for modules already exported.

### Tier 1 — automation expansion

9. **Concurrent serve + global max-in-flight cap**
   - Upgrade: move from one-connection-at-a-time to bounded concurrent serving; enforce global
     `FXRUN_MAX_IN_FLIGHT` alongside per-target single-flight.
   - User involvement: operator chooses cap; no per-job user action unless capacity is exhausted.

10. **Intra-job fan-out / amplification cap**
    - Upgrade: when kernels can emit sub-dispatches, track parent job id and cap child dispatch count
      per route/class.
    - User involvement: operator sets caps; exceeded jobs get recovery/escalation.

11. **Rule-citation audit schema**
    - Upgrade: every policy rejection carries `denied_by={gate, rule_id, configured_value}`.
    - User involvement: desktop/CLI can filter “why was this blocked?” without log archaeology.

12. **Freshness and required-check input seams**
    - Upgrade: accept App-signed `head_sha_is_tip` and `required_checks_green` facts, then gate stale
      or unverified work before delegation.
    - Dependency: `flexnetos_github_app` must derive/sign these facts.

### Tier 2 — result/rollback lifecycle

13. **Structured kernel result/status contract**
    - Upgrade: child writes a status JSON next to `FXRUN_COST_FILE`; missing status fails closed unless
      route opts into synthesized default.

14. **Holdout output-coverage gate**
    - Upgrade: compare JobSpec intent/request fields with result summary; fail/hold if the kernel
      returns success without addressing the requested work.

15. **Pre-dispatch outcome simulation**
    - Upgrade: forecast success probability and projected cost before delegation using history + route
      class; optionally defer/reject above thresholds.

16. **Reversibility classification + pre-action checkpoint**
    - Upgrade: classify each dispatch reversible/irreversible, create restore point for reversible
      mutations, and require higher authority for irreversible work.

17. **Pre/post hook middleware with veto**
    - Upgrade: ordered hooks around dispatch; non-zero pre-hook vetoes, post-hook can classify output
      without masking the original failure.

18. **Dispatch lifecycle FSM and resume journal**
    - Upgrade: formal per-job states from admitted -> delegated -> returned -> verified/blocked;
      persist enough to explain/recover after dispatcher crash.

19. **Safe reclamation / workspace reuse gates**
    - Upgrade: before deleting or adopting a workspace, prove ownership, no active refs, persisted
      output, and landed products.
    - Dependency: reusable tmpfs-worktree path.

## Full agent automation story

1. **Trigger** — A GitHub webhook, scheduled loop, or user-issued command asks for work.
2. **Control-plane preparation** — The App/weave builds the JobSpec, picks the route class, derives
   submitter authority, attaches deadlines and any approval grant, then signs the dispatch envelope.
3. **Local admission** — `fxrun-dispatch` receives the frame, verifies it, runs every policy gate, and
   either returns a precise recovery directive or produces a `KernelPlan`.
4. **Delegation** — The runner creates a fresh workspace, injects only named secrets, spawns the
   existing kernel, gives it the JobSpec on stdin, and bounds runtime with deadline + liveness.
5. **Observation** — The runner records cost, risk, route witness, policy decisions, stderr tails, and
   recovery advice in redacted logs.
6. **Autonomous follow-through** — The App/weave consumes the response: retry later for back-pressure,
   re-dispatch after cooldown, open/continue a human approval thread only when required, or mark the
   work done when accepted.
7. **Self-improvement loop** — Gaps from audit/telemetry become backlog tasks; each task is applied
   as a small cycle with tests, CI, PR, merge, and ledger update.

The target end state is “owner not in the clock”: safe work flows without asking; blocked work is
parked with a concrete reason; the system keeps processing other eligible work; and only genuinely
human decisions reach the user.

## User story and communication flow

### Entry points

```text
Desktop app                    CLI/tools                         GitHub
-----------                    ---------                         ------
Open runner dashboard          fxrun doctor                      PR / issue / webhook
Approve / reject held work     fxrun route KIND --agent ...      Checks and PR comments
Re-arm budget/quarantine       fxrun agents                      Semantic title / CI status
Inspect audit timeline         fxrun-dispatch --socket PATH      Actions runner jobs
Configure secrets/kernels      fxrun-actions install/register
```

### Normal no-user path

1. User has already configured the local runner and secrets.
2. A PR/event arrives.
3. App sends a signed job to `fxrun-dispatch`.
4. Runner admits and delegates automatically.
5. Kernel completes; audit log and App response update GitHub/desktop state.
6. User may see a passive notification: “job completed”, but no action is required.

### User-involved paths

| When | User sees | User action | Exit condition |
|---|---|---|---|
| Approval band requires human | Desktop/CLI/GitHub message: job class, repo, risk, reason, approve/reject buttons/command. | Approve grant or reject. | Approved job re-dispatched with grant; rejected job stops with audit entry. |
| Budget exhausted | “Runner halted by budget cap; spend summary; re-arm?” | Raise cap, reset session, or leave halted. | New budget state is applied; queued work resumes or remains parked. |
| Quarantine hit | “Fingerprint failed N times; terminal until reviewed.” | Inspect stderr/audit, fix root cause, clear quarantine/re-arm. | Fingerprint can dispatch again after explicit re-arm. |
| Constitution changed | “Governing file changed; dispatch halted.” | Review diff, accept/reseal or revert. | Constitution resealed; dispatch resumes. |
| Content/authority/target denied | Precise gate + rule id (planned) and remediation hint. | Adjust policy, fix App provenance, or leave denied. | New signed/policy-compliant request passes. |
| Runner install/register | CLI requires `--confirm=true`. | Confirm host/GitHub mutation. | Runner installed/registered or command aborts safely. |

### Communication channels

- **Desktop app**: primary human loop for approvals, re-arm, status timeline, and safe policy edits.
- **CLI**: power-user and automation surface (`fxrun`, `fxrun-dispatch`, `fxrun-actions`).
- **GitHub**: external evidence plane: PR checks, comments, Actions runner registration, semantic PR
  title, CI status.
- **Audit files**: machine-readable source of truth for event replay and support/debugging.
- **App/weave**: orchestration loop that decides retry timers, follow-up dispatches, escalation PRs,
  and cross-repo coordination.

### Exit points

- **Success**: accepted dispatch, kernel success, audit `delegated`, optional cost/status/result.
- **Back-pressure**: retry-after from rate/state gate; no human unless repeated or policy says so.
- **Human gate**: approval/re-arm required; work is parked, not lost.
- **Terminal refusal**: malformed/content/fork/authority/target/quarantine/constitution denial; exact
  rule and recovery path are returned.
- **Infrastructure failure**: spawn/deadline/idle/kernel fatal; recovery says retry or escalate.
