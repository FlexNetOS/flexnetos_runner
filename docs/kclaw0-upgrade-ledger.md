# kclaw0 → flexnetos_runner Upgrade Ledger

The compounding memory of a continuous **research → surface → apply → research** loop that mines
[`drdave-flexnetos/kclaw0`](https://github.com/drdave-flexnetos/kclaw0) (a self-upgrading autonomous
agent) for patterns the **local runner** can adopt. kclaw0's own doctrine: *text > brain* — so the
loop's state lives here, not in a context window.

**Scope rule:** surface only **runner-plane** primitives (the execution plane that hosts the loop).
**Model-router capabilities are out of scope — weave owns them.** The runner selects an agent
backend (PR #4) as a seam weave drives; it never decides *which* model.

Background: `meta/DARK-FACTORY-RESEARCH.md` (the autonomous-loop landscape this fits into).

## Applied

| # | kclaw0 source | Runner upgrade | Where | PR |
|---|---------------|----------------|-------|----|
| 1 | `scripts/loop-detection.js` (+ `fingerprint.js`) — "4 identical tool calls → loop" + SHA-256 keying | **`LoopGuard` circuit breaker**: trip fail-closed when the same *semantic* job (SHA-256 of `JobKind`, excluding volatile id) recurs ≥`threshold` within a `window` of dispatches. Dispatcher consults it before routing; tunable via `FXRUN_LOOP_WINDOW`/`FXRUN_LOOP_THRESHOLD`; default 4-in-8. | `runner-core::loopguard`, wired in `runner-dispatch` | #5 (merged) |
| 2 | `scripts/dark-factory.js::enforceBudget` + `scripts/survival.js` — hard budget cap / halt-at-zero | **`Governor` dispatch budget**: a bounded-autonomy kill-switch admitting at most `FXRUN_DISPATCH_BUDGET` dispatches per server lifetime, then refusing fail-closed (re-arm to continue). Unlimited by default (behaviour-preserving). Volume complement of the breaker; checked after it so a refused-loop job costs no budget. | `runner-core::governor`, wired in `runner-dispatch` | #6 (merged) |
| 3 | `scripts/event-system.js` — structured NDJSON action log (fixed event vocabulary, unique ids) | **`EventSink` dispatch audit trail**: every terminal decision (verified/fork-rejected/loop-tripped/budget-denied/delegated/kernel-failed) emitted as a one-line NDJSON [`DispatchEvent`] keyed by job fingerprint + correlation id — the audit/lineage requirement (research Goal G). `runner-core` stays I/O-free (trait + NDJSON + `NullSink`); a `FileSink` over `FXRUN_EVENT_LOG` lives in the binary. Off by default. | `runner-core::events`, `FileSink` in `runner-dispatch` | #7 (merged) |
| 4 | `scripts/cost-tracker.js` + `dark-factory.js::enforceBudget` (caps `usedTokens` **and** `usedUsd`) | **`atc → runner` cost seam + cost-aware `Governor`**: `KernelInvoker::invoke` now returns the job's [`JobCost`] (tokens + micro-USD) that `atc` measured; `Governor` became multi-dimensional (jobs **/** tokens **/** USD caps) with `admit()` (pre-dispatch gate) + `charge()` (post-dispatch, from the report). Caps via `FXRUN_DISPATCH_BUDGET` / `FXRUN_TOKEN_BUDGET` / `FXRUN_USD_MICROS_BUDGET`. **Fail-open**: unmeasured jobs ([`JobCost::ZERO`], today's dry-run) charge nothing, so cost caps are inert until `atc` reports — defining the seam without needing `atc` yet. Cost also lands in the audit log. | `runner-core::cost` + `runner-core::governor`, wired in `runner-dispatch` | #8 (merged) |
| 5 | **prior-art:** `Conway-Research/automaton` (protected, `chmod 0444`, git-versioned constitution files; "no override") + `dark-factory.js::verifyImmutability` (SHA-256 of `MISSION.md`/`FACTORY_RULES.md`/`CLAUDE.md`) | **`Constitution` immutability gate**: the dispatcher seals SHA-256 of its own governing files (`FXRUN_CONSTITUTION`) at startup and re-checks them **before every dispatch**; a mid-run change/deletion refuses **all** dispatch — the FIRST gate (above `verify`), so an agent can't weaken its own guardrails (the Darwin-Gödel Machine *deleted its own reward markers*; research §5). `runner-core` I/O-free (closure-injected reader); off by default. | `runner-core::constitution`, wired in `runner-dispatch` | #9 (merged) |
| 6 | **prior-art:** `strongdm/attractor` VALIDATE phase (refuse on structural ERROR before real work) + `retry_target`/`fallback_retry_target`/`wait.human` edges (declared recovery, not ad-hoc loops) | **Structural lint + declarative recovery routing.** Two seams: (a) a **`lint`** gate right after authentication refuses a malformed JobSpec (empty/`owner/name`-invalid repo, blank `head_sha`, `pr_number == 0`, blank ids) *before* a kernel is touched — a late, opaque kernel failure becomes an early, precise rejection. (b) **`recovery`** turns each failed dispatch into *advice carried back to the orchestrator*: transient `KernelFailed` → **retry-with-exponential-backoff** up to `FXRUN_MAX_RETRIES` (per-fingerprint `RetryLedger`, cleared on clean delegation), then **escalate-to-human** (open a review PR); `LoopTripped` / `Malformed` → escalate immediately (un-retryable). Directive rides back on `DispatchResponse.recovery` + the audit log. Delegate-only: the runner *recommends*, weave/App acts (owns the timer + escalation PR). | `runner-core::lint` + `runner-core::recovery`, wired in `runner-dispatch` (+ `DispatchResponse`/`DispatchEvent` fields, `Outcome::Malformed`) | #10 (merged) |
| 10 | **prior-art:** `coleam00/Archon` "fail → delete the worktree, zero residue" | **Isolated-workspace teardown guarantee.** A RAII guard (`JobWorkspace`) whose `Drop` runs the injected cleanup **exactly once on every exit path** — clean return, `?` early-return on a kernel error, or panic — so a failed/breaker-killed job leaves **zero residue**. Explicit `teardown()` returns a `TeardownReport` (ran? residue?); cleanup that *itself* fails is surfaced as auditable residue, never silently dropped. `runner-core` stays I/O-free (cleanup is a closure; `WorkspaceProvider` seam). The dry-run invoker now acquires a real temp-dir workspace per job via a binary `TempDirProvider` (P3 swaps in a tmpfs worktree — same contract). | `runner-core::workspace` (`JobWorkspace`, `WorkspaceProvider`, `TeardownReport`), `TempDirProvider` in `runner-dispatch` | feat/workspace-cleanup |
| 8 | **prior-art:** `coleam00/Archon` `ApprovalNode` / `interactive: true` + `strongdm/attractor` `wait.human` hexagon | **Human-approval admission gate.** An operator opts job *classes* into an approval band (`FXRUN_APPROVAL_BANDS=ci,review,agent,cycle`); a job in an enabled band is **held** (not delegated) unless the frame carries a valid **approval grant** — an HMAC (under the dispatch key) over the job *fingerprint*, bound to the approver, so it can't be replayed onto a different job and can't be forged without the key. Gate sits **before** the breaker/budget so a held job consumes neither the loop window nor the budget; a hold escalates via the recovery layer (`wait.human`). `Outcome::ApprovalRequired`, `FailureKind::ApprovalRequired`, grant on the `DispatchRequest` envelope (not the signed spec — an out-of-band orchestrator fact). Delegate-only: the runner holds + surfaces the request; a human/the orchestrator approves and re-dispatches. Inert unless a band is enabled. | `runner-core::approval` (`ApprovalPolicy`) + `runner-core::wire::Approval`, wired in `runner-dispatch` | #12 (merged) |
| 9 | **prior-art:** `Conway-Research/automaton` separate `policy_decisions` table + git lineage (guardrail decisions auditable apart from execution) | **`policy_decisions` audit stream** (extends the cycle-3 audit log). Each [`Outcome`] is classified `EventCategory::{Policy, Execution}`: *Execution* = the kernel actually ran (`Delegated`/`KernelFailed`); *Policy* = every admission/guardrail decision (constitution, frame auth, lint, fork, approval, breaker, budget). A binary-side `RoutingSink` writes **all** events to `FXRUN_EVENT_LOG` (unchanged) and **policy-only** events to a distinct `FXRUN_POLICY_LOG`, so guardrail tampering is auditable on its own stream (pairs with the constitution gate, row 5). `runner-core` stays I/O-free (just the classifier); off by default. | `runner-core::events` (`EventCategory`, `Outcome::category`), `RoutingSink` in `runner-dispatch` | feat/policy-audit-stream |
| 7 | **prior-art:** `Conway-Research/automaton` 5-tier balance ladder (healthy → conserving → critical → distress → dead) + grace-before-dead | **Survival tiers + debounced halt** (extends the `Governor`). (a) **`SurvivalTier`** (full → conserving@75% → distress@90% → halted) classifies how close the *worst* capped dimension is to its ceiling — read-only **observability** the operator/weave acts on *before* the wall (the runner still only hard-stops at the cap; the *model-downgrade* response stays weave's). Surfaced in the audit log: a degraded-tier delegation carries a `survival tier: …` note. (b) A **debounced floor** (`FXRUN_BUDGET_GRACE`): when a cap is first met, allow up to `grace` further "distress" admits before refusing all dispatch — avoids latching the kill-switch on a single tiny overshoot / in-flight job. `grace = 0` (default) is the exact strict cliff (behaviour-preserving). | `runner-core::governor` (`SurvivalTier`, `Budget::grace`, `tier()`), wired in `runner-dispatch` | #11 (merged) |
| 11 | **prior-art:** `Conway-Research/automaton` child lifecycle `… → unhealthy → recovering → dead` + `strongdm/attractor` terminal-failure state (a unit that keeps failing the same way is moved *terminal*, not retried forever) — surfaced independently by the cycle-11 deep-research sweep. | **Cross-dispatch quarantine ledger** — the *enforcement teeth* behind recovery's escalate advice. The breaker trips on identical-dispatch *volume* (and self-recovers as it ages out); recovery counts `KernelFailed` attempts but only flips its *advice* retry→escalate (nothing stops the orchestrator re-firing doomed work). Quarantine closes that gap: once a fingerprint fails at the kernel `FXRUN_QUARANTINE_THRESHOLD` times it latches **terminal**, and every subsequent dispatch is **refused at admission (before breaker/budget/kernel)** until an operator re-arms — same kill-switch doctrine as the budget governor. A clean delegation resets the count + releases it, so only *persistent same-way* failure latches (a transient blip recovers). New gate after `approval`, before `breaker`. `Outcome::Quarantined` (Policy category) + `FailureKind::Quarantined` (escalate). Pure runner-core; **opt-in** (threshold 0 = off, behaviour-preserving). | `runner-core::quarantine` (`QuarantinePolicy`, `QuarantineLedger`), wired in `runner-dispatch` | #16 (merged) |
| 12 | **prior-art (triple-converged):** `strongdm/attractor` `timeout` node attribute (*"the engine may interrupt handlers exceeding it"*) + `coleam00/Archon` `GIT_OPERATION_TIMEOUT_MS` per-op ceilings + `kclaw0` `docker-exec.js` `defaultTimeout`/`dockerStop` — the **same** primitive named independently by all three cycle-12 research agents. | **Per-job wall-clock deadline** — the one failure axis the existing guards miss: the breaker catches *loops*, the governor caps *cost/volume*, quarantine latches *repeat-failure*, but none bounds a *single, non-looping, in-budget* job that simply **hangs**. The runner now bounds a delegation by time: `DeadlinePolicy` (operator ceiling `FXRUN_DEFAULT_DEADLINE_SECS`) ∧ a per-job request on the `DispatchRequest` envelope (like the approval grant), combined fail-closed as `effective = min(cap, requested)` so a request can only *shorten*, never exceed the cap. A `thread::scope` watchdog wraps the delegate step (engaged only when a deadline is set — the default path is byte-for-byte unchanged); on expiry the delegation is abandoned + the workspace reclaimed (#10), and the timeout routes through recovery (`FailureKind::DeadlineExceeded`, retryable → retry-with-backoff → escalate) and the quarantine ledger (#11) like any failure. `Outcome::DeadlineExceeded` (Execution category). Seam-first: P3's subprocess invoker additionally hard-kills its child at the deadline (attractor's "interrupt" / Archon's `dockerStop`); the runner stays delegate-only (bounds + classifies the wait). Off by default (no cap, no request → watchdog disengaged). | `runner-core::deadline` (`DeadlinePolicy`) + `wire::DispatchRequest::deadline_secs` + `Outcome`/`FailureKind::DeadlineExceeded`, watchdog in `runner-dispatch` | feat/dispatch-deadline |

### Cycle-2 research note — kclaw0 `dark-factory.js` is a governance engine
Admission sequence: **immutability → budget → state-machine → holdout**. Mapping to the runner plane:
- **immutability** (SHA-256 of `MISSION.md`/`FACTORY_RULES.md`/`CLAUDE.md`; trip if changed) → the
  "agent-immutable constitution/oracle". Partly the App's `is_protected()` denylist; a runner-side
  *constitution-fingerprint* gate is a future candidate (needs the file set as a dispatch input).
- **budget** → **applied this cycle** (the `Governor`).
- **holdout** (`validateHoldout`: keyword-coverage of the issue vs the implementation) → the
  *defining* dark-factory signature, but it's a CI/eval gate (loop_lib / required-check aggregation),
  **not** runner-core. Track as a required-status the runner's autonomy gate consumes — not a
  runner-core primitive.
- `verify-commit.js` (tests-must-pass pre-commit) → backpressure; same "lives in CI/loop_lib, the
  runner *requires* the green status" placement. Kept in *Surfaced*.

## Surfaced — candidates for future cycles (not yet applied)

Ranked by runner-plane fit. Each names the kclaw0 source and the in-scope runner analogue. As of the
cycle-6→10 batch, **every *unblocked, in-scope* runner-plane candidate has been applied** — the items
below are now either applied (✓), blocked on another component, or out-of-scope (weave/CI).

- ✓ **`cost-tracker.js`** → **APPLIED** as the `atc → runner` cost seam (cycle 4, row 4) — `JobCost`
  on the invoker return, surfaced in the audit log + charged to the cost-aware governor.
- ✓ **`survival.js`** (credit tiers → halt) → **APPLIED** as the `Governor` job/token/USD kill-switch
  (cycle 2, row 2) and the `SurvivalTier` ladder + debounced halt floor (cycle 7, row 7). The
  model-*downgrade* half stays weave's, as scoped.
- ⛔ **`verify-commit.js`** (tests-must-pass before a job counts) → a "merge only if verified"
  backpressure gate. **Out of runner-core:** the green status is produced by CI/loop_lib; the runner
  *requires* it as a status input, it doesn't compute it. Revisit when the App exposes a required-check
  aggregate the dispatcher can consume (same shape as the dark-factory holdout, research §5).
- ⛔ **`staleness.js`** (refuse a JobSpec whose `head_sha` is no longer the PR tip) → **blocked on a
  head-tip freshness seam from `flexnetos_github_app`.** The runner can't know the current tip without
  the App telling it; apply once that input exists (then it's a cheap pre-`route` gate).
- 🔀 **`steering-queue.js` / `followup-queue.js`** (operator steering / follow-up between jobs) →
  **out of scope (weave/atc orchestration).** Watch for a runner seam if weave needs the runner to
  honour an inter-job signal.
- ✓/▢ **`checkpoint.js`** → job-level checkpointing is covered by the `handoff` kernel the runner
  delegates to; no runner-core primitive needed unless a *dispatch-level* checkpoint seam is found
  missing in P3.

**Net:** the actionable backlog (prior-art batch #2–#6, below) is fully applied. What remains is gated
on other components (`verify-commit`, `staleness`) or owned elsewhere (`steering-queue` → weave) — none
is a runner-plane primitive that can be built today.

### Prior-art batch (widened beyond kclaw0 to the sources it names — ranked, runner-plane lens)
From the phase-3 research sweep of kclaw0's named prior-art (cited in `meta/DARK-FACTORY-RESEARCH.md`):
- ~~**#2 Goal-gate + retry-target rollback routing**~~ → **APPLIED (cycle 6, `feat/recovery-routing`).**
  Shipped as `runner-core::lint` (structural VALIDATE gate) + `runner-core::recovery` (retry-with-backoff
  → escalate-to-human directive on `DispatchResponse`). See *Applied* row 6.
- ~~**#3 Survival-tier debounced halt**~~ → **APPLIED (cycle 7, `feat/survival-tiers`).** Shipped as
  `SurvivalTier` (full → conserving@75% → distress@90% → halted) + `Budget::grace` debounced floor on
  the `Governor`; degraded tier surfaced in the audit log. See *Applied* row 7. Model-*downgrade*
  stays weave's, as scoped.
- ~~**#4 Human-approval gate**~~ → **APPLIED (cycle 8, `feat/approval-gate`).** Shipped as
  `runner-core::approval::ApprovalPolicy` (opt-in bands) + `wire::Approval` (fingerprint-bound HMAC
  grant), held before breaker/budget, `Outcome::ApprovalRequired`. See *Applied* row 8.
- ~~**#5 Isolation-cleanup-on-fail**~~ → **APPLIED (cycle 10, `feat/workspace-cleanup`).** Shipped as
  `runner-core::workspace::JobWorkspace` (RAII teardown guaranteed on every exit incl. fail/panic, with
  residue reporting) + `WorkspaceProvider` seam; the dry-run invoker acquires a real temp-dir workspace
  per job. See *Applied* row 10. The P3 invoker swaps the temp dir for a tmpfs worktree (same contract).
- ~~**#6 `policy_decisions` audit stream**~~ → **APPLIED (cycle 9, `feat/policy-audit-stream`).** Shipped
  as `EventCategory::{Policy,Execution}` + a binary `RoutingSink` teeing policy events to a distinct
  `FXRUN_POLICY_LOG`. See *Applied* row 9.

**Out of scope (weave/atc/CI), confirmed by the sweep:** `pi-subagents` reviewer-loop + per-agent model
overrides (atc/weave); `Conway-Research/skills` progressive disclosure (context-engineering);
`Understand-Anything` / `mempalace` / `chroma` / `second-brain-starter` (memory/KG/vector — weave/atc).

### Cycle-11 deep-research sweep (3 parallel agents: kclaw0 + Archon + attractor/automaton)
A fresh, deeper sweep — past the exhausted #2–#6 batch — re-read the source of all four targets for
mechanisms not yet adopted. Strong **cross-source convergence** (the signal that an item is real, not
a single repo's quirk). Ranked, runner-plane lens; **#11 (quarantine) applied this cycle**:

- ✓ **Cross-dispatch quarantine** (automaton child `→ dead` lifecycle + attractor terminal-failure) →
  **APPLIED (cycle 11, `feat/quarantine-ledger`).** See *Applied* row 11.
- ✓ **Per-job wall-clock deadline / hung-delegate timeout** (triple-converged: kclaw0 `docker-exec.js`
  + Archon `GIT_OPERATION_TIMEOUT_MS` + attractor `timeout` node) → **APPLIED (cycle 12,
  `feat/dispatch-deadline`).** Shipped as `runner-core::deadline::DeadlinePolicy` + a per-job
  `DispatchRequest::deadline_secs` (envelope, `effective = min(cap, requested)`) + a `thread::scope`
  watchdog that abandons a hung delegation, routing the timeout through recovery
  (`FailureKind::DeadlineExceeded`) + quarantine (#11). `Outcome::DeadlineExceeded`. See *Applied*
  row 12. The actual subprocess hard-kill is the P3 invoker's (seam-first).
- ▶ **Audit-path secret redaction** (Archon `repo.ts` scrubs the auth token from every error *before*
  it is classified/logged/returned). The runner handles HMAC keys + approval-grant tokens; a
  `redact(text, secrets)` seam invoked by the audit writer + the UDS error responder ensures key
  material never lands in `FXRUN_EVENT_LOG` / `FXRUN_POLICY_LOG` / a reply. Cheap, high-value, fully
  in-scope, fully live-verifiable. **Queued.**
- ▷ **Windowed rate-limit + per-route error cooldown** (automaton hourly/daily caps + "5-min backoff
  on error") — extend the `Governor` from cumulative caps to rolling-window counters (jobs/min,
  jobs/hour, `max_in_flight`) + a post-failure per-route cooldown stamp. In-scope Governor extension;
  distinct from the retry backoff (which is per-job advice). **Queued.**
- ▷ **Pre-dispatch content/injection scan** (Archon `marketplace-security-scan.ts`: severity-graded
  regex pattern-banks, scan/decide split, fail-closed on critical/high; kclaw0 `path-simulator.js`
  risk scoring). Lighter for a delegate-only router (the JobSpec carries no source — only `prompt_ref`
  / `task_id` strings), so the in-scope slice is an **injection-pattern scan of the spec's string
  fields** → `ScanReport{severity, findings}` → fail-closed reject at/after `lint`. **Queued (lower
  fit; verify the payload surface is worth a gate).**
- ▷ **History-calibrated pre-route risk score** (kclaw0 `path-simulator.js` blends a static base rate
  with the *live* failure rate from the event ledger). Pure advice over seams already present (#3
  audit history + #4 cost); feeds approval/recovery (high predicted-failure → bias conservative).
  Advice-only, novel, but lower urgency than the guards above. **Backlog.**
- ⛔ **Adoption-time workspace ownership verification** (Archon `assertWorktreeOwnership` — refuse to
  *adopt* a worktree not provably bound to this repo/fingerprint). The acquisition-side dual of the
  applied teardown (#10), but **blocked-until-a-reuse-path-exists**: today's `WorkspaceProvider`
  always creates fresh, so there is nothing to adopt yet. Apply alongside the P3 tmpfs-worktree reuse.
- ⊕ **Rule-citation audit schema** (automaton `policy_decisions` 6-category: each denial cites
  *which gate + which rule*). A **refinement** of the applied dual stream (#9): tag each `Policy`
  refusal with `denied_by:{gate, rule_id}` so the stream is queryable by gate. Low-risk increment.
- ▽ **Dispatch resume journal** (attractor checkpoint/resume) — marginal for a single-shot delegate
  router; only the crash-recovery-lineage slice is in scope. Defer unless in-flight durability becomes
  a goal. **Rejected by all three agents:** heartbeat / dead-man's-switch (no persistent loop to
  guard), distill/optimize/ReasoningBank nodes (weave learning), `goal_gate`/holdout (CI/eval), and
  every model-selection mechanism (weave).

**Net after cycle 12:** quarantine (11) + deadline (12) applied. The queued backlog is now
**audit-path secret redaction → windowed rate-limit + per-route cooldown → pre-dispatch
content/injection scan → history-calibrated risk score** — all unblocked, in-scope runner-plane work.
Redaction is the next pick (smallest, security-relevant, fully live-verifiable, no wire/trait change).
Adoption-ownership remains the only newly-surfaced item gated on another component (the P3 reuse path).

## Deferred / out of scope (model-router — weave owns)

- `llm-client.js`, `subagent-profiles.js`, model selection/routing, provider switching (`cc-switch`).
  The runner exposes the `agent` seam (PR #4); weave drives it.

## Method (per cycle)

1. **Research** — read one kclaw0 script/system; note the mechanism.
2. **Surface** — decide the runner-plane analogue (or mark out-of-scope → weave).
3. **Apply** — implement in `runner-core` (+ wire a binary), test, keep CI green, land a PR.
4. **Record** — move the item to *Applied*; add anything newly seen to *Surfaced*.
