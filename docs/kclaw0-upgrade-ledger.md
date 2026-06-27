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
| 12 | **prior-art (triple-converged):** `strongdm/attractor` `timeout` node attribute (*"the engine may interrupt handlers exceeding it"*) + `coleam00/Archon` `GIT_OPERATION_TIMEOUT_MS` per-op ceilings + `kclaw0` `docker-exec.js` `defaultTimeout`/`dockerStop` — the **same** primitive named independently by all three cycle-12 research agents. | **Per-job wall-clock deadline** — the one failure axis the existing guards miss: the breaker catches *loops*, the governor caps *cost/volume*, quarantine latches *repeat-failure*, but none bounds a *single, non-looping, in-budget* job that simply **hangs**. The runner now bounds a delegation by time: `DeadlinePolicy` (operator ceiling `FXRUN_DEFAULT_DEADLINE_SECS`) ∧ a per-job request on the `DispatchRequest` envelope (like the approval grant), combined fail-closed as `effective = min(cap, requested)` so a request can only *shorten*, never exceed the cap. A `thread::scope` watchdog wraps the delegate step (engaged only when a deadline is set — the default path is byte-for-byte unchanged); on expiry the delegation is abandoned + the workspace reclaimed (#10), and the timeout routes through recovery (`FailureKind::DeadlineExceeded`, retryable → retry-with-backoff → escalate) and the quarantine ledger (#11) like any failure. `Outcome::DeadlineExceeded` (Execution category). Seam-first: P3's subprocess invoker additionally hard-kills its child at the deadline (attractor's "interrupt" / Archon's `dockerStop`); the runner stays delegate-only (bounds + classifies the wait). Off by default (no cap, no request → watchdog disengaged). | `runner-core::deadline` (`DeadlinePolicy`) + `wire::DispatchRequest::deadline_secs` + `Outcome`/`FailureKind::DeadlineExceeded`, watchdog in `runner-dispatch` | #17 (merged) |
| 14 | **prior-art:** `Conway-Research/automaton` — hourly/daily call caps + a **"5-minute backoff on error"** per-endpoint penalty. | **Windowed dispatch rate limit + per-route failure cooldown.** The **timing** axis the other guards miss: the breaker catches *same-job loops*, the governor caps *lifetime* budget, quarantine latches *repeat-failure* — but a burst of *distinct, in-budget, non-looping* jobs in a few seconds sails through all three. New `runner-core::ratelimit::RateLimiter` (clock-injected, so `runner-core` stays clock-free — like `deadline`): a rolling **window cap** (≤`max_per_window` admits per `window_secs`) + a **per-route cooldown** (after a route — a job `class()` `ci`/`review`/`agent`/`cycle` — fails, hold that route for `route_cooldown_secs`; cleared early by a clean delegation). A rate refusal is **not** a job failure: it emits `Outcome::RateLimited` (Policy) + a `RecoveryVerb::Retry` directive at **attempt 0** carrying a **retry-after**, so it never touches the per-fingerprint retry ledger or escalates to a human (a busy runner must not escalate a job). Gate sits before the breaker/budget (a rate-refused job pollutes neither). The dispatcher reads a monotonic clock per connection and injects `now_secs`. Off by default (`FXRUN_RATE_MAX` / `FXRUN_RATE_WINDOW_SECS` / `FXRUN_ROUTE_COOLDOWN_SECS` all 0 → inert). Verified live: with `FXRUN_RATE_MAX=2`, a 3-job burst over a real UDS socket admits 2, refuses the 3rd with `attempt 0/2, back off 60s`. | `runner-core::ratelimit` (`RateLimiter`, `RateLimitPolicy`, `RateDecision`) + `JobKind::class()` + `Outcome::RateLimited`, wired in `runner-dispatch` (clock-injected gate) | #19 (merged) |
| 15 | **prior-art:** `coleam00/Archon` `marketplace-security-scan.ts` — severity-graded regex **pattern banks**, a **scan/decide split**, **fail-closed on critical/high** + `kclaw0` `path-simulator.js` risk scoring. | **Pre-dispatch content / injection scan.** Structural [`lint`] (#6) proves a spec's *shape* (`owner/name`, non-blank); this checks the *safety* of its free-text fields — which the **P3 invoker will interpolate** into a kernel command line / workspace path / audit line. New `runner-core::scan`: a severity-graded pattern bank (`nul-byte`=Critical; `control-char`/`crlf-injection`/`command-substitution`/`path-traversal`=High; `shell-metacharacter`=Medium; `redirection`=Low — plain substring/char-class checks, **no regex dep**) over every string field (`id`, `correlation_id`, `repo`, `head_sha`, `prompt_ref`, `task_id`) → a `ScanReport{findings, max_severity}`. **Scan/decide split** (Archon): `scan()` only *detects* (pure); a separate `ScanPolicy` (`FXRUN_SCAN_BLOCK_SEVERITY=low|medium|high|critical`) decides — refuse fail-closed when the worst finding meets the threshold. Gate sits **after lint, before fork/route**. Hostile content can't be fixed by re-dispatch, so recovery **escalates** (`FailureKind::ContentRejected`, un-retryable — never retries). `Outcome::ContentRejected` (Policy). **Off by default** (seam-first: the acquisition-side guard for the P3 invoker's interpolation, defined before that interpolation exists). Delegate-only: the runner *refuses*; it never sanitizes/rewrites the job. Verified live: `FXRUN_SCAN_BLOCK_SEVERITY=high` admits `task_id=T-1`, refuses `T-1; rm -rf $(ls ~)` (`job.task_id [high] command-substitution`). | `runner-core::scan` (`scan`, `ScanReport`, `ScanPolicy`, `Severity`, `Finding`) + `Outcome`/`FailureKind::ContentRejected`, wired in `runner-dispatch` | #20 (merged) |
| 16 | **prior-art:** `kclaw0` `path-simulator.js` — blends a **static base rate** with the **live failure rate observed from the event ledger** to predict how risky an action is *before* taking it. | **History-calibrated pre-route risk score** — the runner's one **advice-only** signal (every other guard is a hard gate). New `runner-core::risk`: a `RiskLedger` accumulates per-fingerprint `(successes, failures)` across dispatches (NOT cleared on success — calibration needs the whole record), and a `RiskModel` computes a Beta-smoothed failure probability `score = (failures + base·prior)/(total + prior)` — so with **no** history it is exactly the static `base_rate`, and as real evidence accrues it converges on the *observed* rate (the source's "static base blended with live evidence", made statistically honest). Banded Low/Elevated/High. Computed **before** the delegation outcome (predicts, not reflects) and surfaced on the delegated/failed audit event (a structured `risk` field + a `risk: …` detail note) for the orchestrator/weave to act on — it **never blocks** (the soft, continuous companion to the hard breaker/quarantine latches: the *gradient* that lets a consumer react before a latch fires). Pure / clock-free; `RiskPolicy` opt-in via `FXRUN_RISK_ANNOTATE` (off by default → audit stream unchanged). `DispatchEvent` dropped its `Eq` derive (the `f64` score is `PartialEq` only). Delegate-only, orthogonal to model routing. Verified live: with annotation on, a delegated job's real on-disk audit line carries `risk={score:0.10, samples:0, band:low}`; the decision-core test drives 6 failures and watches the band climb Low→High. | `runner-core::risk` (`RiskLedger`, `RiskModel`, `RiskPolicy`, `RiskScore`, `RiskBand`) + `DispatchEvent.risk`, wired in `runner-dispatch` | feat/dispatch-risk-score |
| 17 | **prior-art:** `coleam00/Archon` `executor-shared.ts::classifyError` — a two-axis error classifier with **FATAL patterns checked before TRANSIENT** (so `"unauthorized: process exited with code 1"` is never silently retried); convergent with the retry loops in `kclaw0` + `strongdm/attractor` (surfaced independently by the cycle-16 sweep). | **FATAL-first kernel-error taxonomy.** Until now every kernel error was `FailureKind::KernelFailed` (retryable), so an **auth / permission / config** failure burned the whole retry budget before escalating — re-dispatching a job that could only fail the same way. New pure `recovery::classify_kernel_error(msg)` applies Archon's **FATAL-before-TRANSIENT precedence**: a message matching the fatal bank (`unauthorized`/`forbidden`/`permission denied`/`401`/`403`/`invalid api key`/…) → new un-retryable `FailureKind::KernelFatal` → **escalate-to-human immediately** (attempt 0, no retry-budget spend); anything unrecognized defaults to `KernelFailed` (transient, retry-with-backoff) — the conservative default, since the retry ceiling + quarantine ledger backstop a *persistent* "transient". The audit stream gains `Outcome::KernelFatal` (Execution category) — a **fixed-enum telemetry class** that records the failure *kind* without the (possibly secret-bearing) message text, pairing with redaction (#13). Front-ends the applied recovery routing (#6): the classifier *decides* the kind, recovery *acts* on it. Pure runner-core; behaviour-preserving for unrecognized errors (still retry). Delegate-only, orthogonal to model routing. Verified: a `FatalInvoker` returning `HTTP 401 Unauthorized` drives the real `handle_request` pipeline to escalate at attempt 0 (retry ledger untouched) and audits `KernelFatal`, where a transient error would have retried. | `runner-core::recovery` (`classify_kernel_error`, `FailureKind::KernelFatal`) + `Outcome::KernelFatal`, wired in `runner-dispatch` (classify the `Delegation::Failed` error) | feat/dispatch-error-taxonomy |
| 13 | **prior-art:** `coleam00/Archon` `repo.ts` — **scrubs the auth token out of every error string before it is classified, logged, or returned** (a failing git op can never echo the credential it was handed). | **Audit-path secret redaction.** The dispatcher holds key material (the HMAC dispatch key; in P3 the envctl-injected bearer / approval tokens) and writes two operator-readable surfaces that could otherwise carry it verbatim — the NDJSON audit log (`detail` fields) and the UDS error reply (`DispatchResponse.error`). `Redactor` is the single choke point: it replaces every occurrence of a known secret with `«redacted»` *before* the text reaches either surface. Two egress points: a pure **`RedactingSink`** decorator scrubs each event's free-text `detail` before the file write (structured lineage ids untouched), and **`redact_response`** scrubs the reply `error` before it crosses the socket. The binary builds the redactor from the dispatch key + comma-separated `FXRUN_REDACT_SECRETS`; a `MIN_SECRET_LEN` floor (4) refuses pathological short stand-ins that would mangle incidental text (real envctl keys are 32 bytes). **Behaviour-preserving:** no/absent secret → input returned borrowed, byte-identical (defense-in-depth — today's messages don't splice secrets, the seam guarantees they never can). Pure `runner-core` (Cow-based compute + decorator); delegate-only, orthogonal to model routing. | `runner-core::redact` (`Redactor`, `RedactingSink`) + `Box<dyn EventSink>` impl, wired in `runner-dispatch` (sink wrap + `redact_response`) | #18 (merged) |
| 18 | **prior-art:** `Conway-Research/automaton` `policy-engine.ts::deriveAuthorityLevel` + attractor access-broker + Archon human-vs-agent origin | **Dispatch provenance / authority gate.** The UDS envelope now has an optional `submitter { id, tier }` provenance seam (`guest < agent < maintainer < owner`), and the dispatcher can enforce per-route authority floors with `FXRUN_AUTHORITY_RULES` (for example `cycle=maintainer,agent=owner`) before content/route work is examined. Missing submitter provenance is treated as `guest` once a floor is configured, so privileged routes fail closed; with no rules configured, older App frames remain byte-compatible and pass unchanged. Denials emit fixed telemetry (`Outcome::AuthorityDenied`) plus recovery escalation (`FailureKind::AuthorityDenied`) and never reach a kernel. Delegate-only: the runner verifies the submitted authority tier; the control plane/weave owns deriving it. | `runner-core::authority` (`Submitter`, `AuthorityTier`, `AuthorityPolicy`) + `wire::DispatchRequest::submitter` + `Outcome::AuthorityDenied`/`FailureKind::AuthorityDenied`, wired in `runner-dispatch` and surfaced in `fxrun doctor` | local (cycle 18) |
| 19 | **prior-art:** `Conway-Research/automaton` `createX402DomainAllowlistRule` fail-closed allowlist + attractor egress allowlist | **Delegation-target allowlist.** The dispatcher now has an optional kernel reachability registry (`FXRUN_KERNEL_ALLOWLIST`) for `loop`/`atc`/`hf`/`weave` targets. Unset is behaviour-preserving (all existing routes allowed); set-but-empty is active deny-all; named kernels allow only those endpoints. The gate routes the authenticated job, then refuses disallowed kernels before rate slots, breaker window, budget, or subprocess invocation are touched. Denials emit fixed telemetry (`Outcome::TargetDenied`) plus recovery escalation (`FailureKind::TargetDenied`). This is kernel reachability only — model/vendor selection remains weave/atc-owned. | `runner-core::targets` (`TargetAllowlist`, `TargetDecision`) + `router::Kernel::parse`/`ALL` + `Outcome::TargetDenied`/`FailureKind::TargetDenied`, wired in `runner-dispatch` and surfaced in `fxrun doctor` | local (cycle 19) |
| 20 | **prior-art:** `coleam00/Archon` `getActiveWorkflowRunByPath` / `ConversationLockManager` older-wins locks + kclaw0 `docker-exec` `maxContainers` | **Per-target single-flight mutex.** The buildable half of the concurrency backlog is now typed: a `SingleFlight` ledger acquires a stable mutable-target key (today: normalized repo) and denies competing in-flight work with deterministic older-wins metadata. The dispatcher acquires the target after route/target allowlist and before rate slots, breaker window, budget, or kernel invocation; an RAII permit releases on every terminal path. Denials emit fixed telemetry (`Outcome::SingleFlightDenied`) plus recovery escalation (`FailureKind::SingleFlightDenied`). The global max-in-flight cap remains P3/concurrent-serve-gated because today's server accepts one connection at a time. | `runner-core::singleflight` (`TargetKey`, `SingleFlight`, `FlightLease`) + `Outcome::SingleFlightDenied`/`FailureKind::SingleFlightDenied`, wired in `runner-dispatch` and surfaced in `fxrun doctor` | local (cycle 20) |
| 21 | **prior-art:** `coleam00/Archon` `idle-timeout.ts` + kclaw0 `docker-exec` liveness timeout | **Idle / liveness watchdog.** Wall-clock deadline bounds total runtime; liveness bounds silence. The dispatch envelope now carries optional `idle_timeout_secs`, and operators can set `FXRUN_IDLE_TIMEOUT_SECS`; the effective idle timeout is the tighter value. The subprocess invoker pipes stdout/stderr when liveness is active, refreshes activity on each yielded byte, and kills a silent child as `Delegation::IdleTimedOut`, routing it through retry/quarantine recovery as `Outcome::IdleTimeout` / `FailureKind::IdleTimeout`. Off by default, so legacy execution remains unchanged. | `runner-core::liveness` (`LivenessPolicy`) + `wire::DispatchRequest::idle_timeout_secs` + `Outcome::IdleTimeout`/`FailureKind::IdleTimeout`, enforced in `SubprocessInvoker` and surfaced in `fxrun doctor` | local (cycle 21) |
| 22 | **prior-art:** attractor `select_edge` deterministic total order (`weight` + stable id) | **Deterministic route-selection contract.** Routing now goes through an explicit `RouteCandidate` selector with a stable total order: `weight DESC, route_id ASC`. Every `KernelPlan` carries the selected `route_id` and `route_weight` witness, and accepted audit details record the selected route. Today each job kind still has one candidate, so behavior is unchanged; the contract is ready for future multi-eligible kernels without nondeterminism. | `runner-core::router` (`RouteCandidate`, `select_route`, `KernelPlan::route_id`/`route_weight`), wired in `runner-dispatch` audit detail and surfaced in `fxrun doctor` | local (cycle 22) |
| 24 | **Cycle-23 audit backlog:** ledger/docs drift guard | **Forge-loop docs drift guard.** A new `fxrun forge-loop docs-drift` check scans the upgrade ledger for applied exported features that are still documented as queued/backlog work, then fails CI before stale governance state can merge. This directly closes the state-gate drift that prompted the audit item and makes every future loop update prove docs consistency in the automated gate set. | `runner-cli::forge_loop` (`docs-drift`) + CI `Forge-loop docs drift guard` step | local (cycle 24) |
| 25 | **Cycle-16 audit backlog:** rule-citation audit schema | **Policy denial rule citations.** Every `DispatchEvent` for a policy refusal now carries additive `denied_by { gate, rule_id }` metadata, so the policy stream can be queried by the exact guardrail and stable rule that blocked dispatch. Execution outcomes omit the field, preserving clean delegation output. | `runner-core::events` (`DeniedBy`, `Outcome::denied_by`, `DispatchEvent.denied_by`) | local (cycle 25) |
| 26 | **Cycle-04 isolated batch:** forge-loop artifact collision audit | **Subsecond forge-loop artifact labels.** Cycle artifact directories now include fixed-width nanoseconds in addition to epoch seconds, so rapid repeated dry runs no longer collide into the same `_work/forge-loop/cycle-*` directory. The label helper is unit-tested with a deterministic timestamp fixture. | `runner-cli::forge_loop` (`timestamp_label_for`) | local (cycle 04) |
| 27 | **Cycle-05 isolated batch:** forge-loop artifact contract audit | **Cycle manifest artifact.** Each `fxrun forge-loop run` artifact directory now records a typed `cycle-manifest.json` with the requested goal, single-cycle `once` setting, auto-merge intent, strict-upgrade flag, and the exact required phase order. This gives schedulers and post-run audit tooling a deterministic contract to verify before trusting a cycle artifact. | `runner-cli::forge_loop` (`CycleManifest`) | local (cycle 05) |
| 28 | **Cycle-06 isolated batch:** forge-loop metrics evidence audit | **Eval metrics validation.** `fxrun forge-loop eval --metrics` now rejects impossible retry evidence before scoring a run, so malformed metrics cannot make a self-upgrade decision look cleaner than the run record supports. The first invariant caps retry count at a reviewable ceiling and reports the invalid field in the parser error. | `runner-cli::forge_loop` (`validate_eval_input`) | local (cycle 06) |
| 29 | **Cycle-07 isolated batch:** nested Codex prompt boundary audit | **Single-cycle prompt contract + deterministic PR title.** The Codex invocation prompt now explicitly tells the nested agent not to start another cycle and, when the goal names a cycle number, derives the conventional PR title (`chore: forge loop cycle NN`) from that goal. This prevents resumed batches from accidentally cascading past the isolated cycle and preserves a predictable publish surface. | `runner-cli::forge_loop` (`cycle_prompt`, `cycle_pr_title`) | local (cycle 07) |
| 30 | **Cycle-08 isolated batch:** forge-loop artifact publish-contract audit | **Manifest PR-title witness.** Each cycle manifest now records the deterministic PR title derived from the goal, matching the nested Codex prompt. This lets schedulers and post-run audit tooling verify the intended publish surface from `cycle-manifest.json` without scraping prompt text. | `runner-cli::forge_loop` (`CycleManifest.pr_title`) | local (cycle 08) |
| 31 | **Cycle-09 isolated batch:** nested Codex prompt integrity audit | **Manifest prompt hash witness.** Each cycle manifest now records the SHA-256 of the exact nested Codex prompt produced from the goal and auto-merge setting, so post-run audit tooling can verify that the prompt contract sent to Codex matches the artifact contract without storing or scraping the full prompt. | `runner-cli::forge_loop` (`CycleManifest.prompt_sha256`) | local (cycle 09) |
| 32 | **Cycle-10 isolated batch:** forge-loop manifest verification audit | **Eval manifest contract verifier.** `fxrun forge-loop eval --manifest` now parses `cycle-manifest.json` before scoring and rejects mismatched PR titles, forged prompt hashes, non-isolated runs, disabled strict-upgrade mode, or phase-order drift. This closes the cycle-09 witness loop by making the recorded prompt hash an enforceable post-run audit guard instead of passive metadata. | `runner-cli::forge_loop` (`parse_cycle_manifest`, `validate_cycle_manifest`, `EvalArgs.manifest`) | local (cycle 10) |
| 33 | **Post-batch runner hygiene gap:** local self-hosted check pressure | **Forge-loop runner-health diagnostic.** `fxrun forge-loop runner-health --checks-json <gh-pr-view.json>` consumes `gh pr view --json statusCheckRollup` output and flags pending local self-hosted required checks (`Local Linux CI`, `Semantic PR Title`) before a harness waits blindly on branch protection. This turns the cycle-10 queue stall into a machine-checkable pre-wait diagnostic with concrete runner-hygiene guidance. | `runner-cli::forge_loop` (`RunnerHealthReport`, `classify_runner_health`, `runner-health`) | local (cycle 11) |
| 34 | **Codex forge-loop harness cycle 12:** branch-protection completeness audit | **Runner-health missing-check detector.** `fxrun forge-loop runner-health` now reports required local checks that are absent from `gh pr view --json statusCheckRollup`, so a loop can distinguish a clear runner queue from a PR whose required local workflows never scheduled or have not materialized yet. | `runner-cli::forge_loop` (`REQUIRED_LOCAL_CHECKS`, `RunnerHealthReport.missing_local_checks`) | local (cycle 12) |
| 35 | **Codex forge-loop harness cycle 13:** runner-health contract observability | **Required-check witness.** `forge-loop runner-health` and `doctor --json` now expose the authoritative local required-check set, so schedulers can compare GitHub rollup data against the harness contract without scraping code or prose. | `runner-cli::forge_loop` (`RunnerHealthReport.required_local_checks`, doctor JSON) | local (cycle 13) |
| 36 | **Codex forge-loop harness cycle 14:** manifest compatibility audit | **Cycle-manifest schema witness.** `cycle-manifest.json` now records a schema version and the manifest verifier rejects unsupported future versions while defaulting legacy manifests to v1, giving post-run tooling a stable compatibility guard. | `runner-cli::forge_loop` (`CycleManifest.schema_version`, `CYCLE_MANIFEST_SCHEMA_VERSION`) | local (cycle 14) |
| 37 | **Codex forge-loop harness cycle 15:** research provenance audit | **Research-sources artifact.** Every `forge-loop run` artifact directory now writes `research-sources.json`, preserving the exact source contract used for self-upgrade research alongside the manifest, invocation, events, and evaluation evidence. | `runner-cli::forge_loop` (`run`, `research_sources`) | local (cycle 15) |
| 38 | **Codex forge-loop harness cycle 16:** gate-contract audit | **Machine-readable required gates.** `forge-loop doctor --json` and `self-upgrade --dry-run` now export the exact required local gate commands, so schedulers and handoffs can consume the harness gate contract without copying prose. | `runner-cli::forge_loop` (`REQUIRED_GATE_COMMANDS`, doctor/self-upgrade JSON) | local (cycle 16) |
| 39 | **Codex docs/config upgrade audit:** advanced config + GitHub Action surfaces | **Forge-loop component audit.** `fxrun forge-loop components-audit --json` now inventories the Codex loop surfaces requested for upgrade — prompt, project config, hooks, rules, custom agents/subagents, skills, and GitHub workflow tools — and reports missing repo-local components before follow-up config changes claim completion. | `runner-cli::forge_loop` (`components-audit`, `expected_loop_components`) | local (component audit) |
| 40 | **Codex docs/config upgrade:** repo-local Codex surfaces | **Project Codex config, hooks, rules, and custom auditor agent.** The repo now carries `.codex/config.toml` with model/effort/sandbox defaults, lifecycle hooks for session readiness and stop summaries, an executable rules file with inline examples, a read-only `forge-loop-auditor` custom agent, and a GitHub Action prompt mirror under `.github/codex/prompts/`. This closes the component gaps surfaced by `components-audit`. | `.codex/config.toml`, `.codex/hooks.json`, `.codex/hooks/*`, `.codex/rules/forge-loop.rules`, `.codex/agents/forge-loop-auditor.toml`, `.github/codex/prompts/forge-loop.md` | local (config surfaces) |
| 41 | **Codex GitHub Action tool surface:** manual forge-loop action | **Codex Action workflow.** The repo now includes `.github/workflows/codex-forge-loop.yml`, a manual `workflow_dispatch` workflow using `openai/codex-action@v1` with documented `prompt-file`, `codex-args`, `model`, `effort`, `sandbox`, `output-file`, and `safety-strategy` controls. `components-audit --strict` now requires this tool surface. | `.github/workflows/codex-forge-loop.yml`, `runner-cli::forge_loop` (`codex-github-action` component) | local (codex action tool) |
| 42 | **Codex loop skill upgrade:** config/action source coverage | **Forge-loop research skill now requires Codex config/action docs.** The skill now explicitly includes OpenAI Codex advanced config and GitHub Action docs, requires component/config inventory output, and tells future config changes to update `components-audit` plus CI. A unit test guards those skill requirements. | `.agents/skills/forge-loop-research/SKILL.md`, `runner-cli::forge_loop` (`forge_loop_skill_references_codex_config_and_action_docs`) | local (skill docs guard) |

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
- ✓ **Audit-path secret redaction** (Archon `repo.ts` scrubs the auth token from every error *before*
  it is classified/logged/returned) → **APPLIED (cycle 13, `feat/audit-redaction`).** Shipped as
  `runner-core::redact::Redactor` (Cow-based, longest-first, `MIN_SECRET_LEN` floor) + the pure
  `RedactingSink` decorator (scrubs audit `detail`) + binary `redact_response` (scrubs the reply
  `error`); redactor built from the dispatch key + `FXRUN_REDACT_SECRETS`. See *Applied* row 13.
  Verified end-to-end over a real UDS socket + real on-disk audit file (a kernel error embedding a
  secret reaches neither the wire reply nor the log).
- ✓ **Windowed rate-limit + per-route error cooldown** (automaton hourly/daily caps + "5-min backoff
  on error") → **APPLIED (cycle 14, `feat/dispatch-rate-limit`).** Shipped as a *separate*
  clock-injected `runner-core::ratelimit::RateLimiter` (NOT a Governor extension — the Governor is
  deliberately clock-free; a sibling module keeps it pure, mirroring how `deadline` stays clock-free):
  rolling **window cap** + **per-route cooldown**, `Outcome::RateLimited`, attempt-0 retry-after
  directive that never escalates. `max_in_flight` deferred (the server is single-connection today —
  no concurrency to bound; revisit if P3 serves concurrently). See *Applied* row 14. Verified live
  over a real socket (2/60s cap → 3-job burst refuses the 3rd).
- ✓ **Pre-dispatch content/injection scan** (Archon `marketplace-security-scan.ts` + kclaw0
  `path-simulator.js`) → **APPLIED (cycle 15, `feat/dispatch-content-scan`).** Shipped as
  `runner-core::scan` — a severity-graded pattern bank over the spec's free-text fields,
  `ScanReport{findings, max_severity}`, scan/decide split via `ScanPolicy`
  (`FXRUN_SCAN_BLOCK_SEVERITY`), fail-closed after `lint`, `Outcome`/`FailureKind::ContentRejected`
  (escalates). The payload surface IS worth a gate: those strings are interpolated into the P3
  invoker's command line / paths. See *Applied* row 15. Verified live (block ≥ high refuses a
  `$(...)` task_id).
- ✓ **History-calibrated pre-route risk score** (kclaw0 `path-simulator.js` blends a static base rate
  with the *live* failure rate from the event ledger) → **APPLIED (cycle 16, `feat/dispatch-risk-score`).**
  Shipped as `runner-core::risk` — a per-fingerprint `RiskLedger` + Beta-smoothed `RiskModel` →
  banded `RiskScore` surfaced (advice-only, never blocks) on the audit event. See *Applied* row 16.
  Verified live (audit line carries the risk annotation).
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

**Net after cycle 16 — SURFACED BACKLOG EXHAUSTED.** quarantine (11) + deadline (12) + secret
redaction (13) + windowed rate-limit (14) + content/injection scan (15) + history-calibrated risk
score (16) applied. **Every unblocked, in-scope runner-plane candidate from the cycle-11 sweep is now
shipped.** What remains is gated on another component (adoption-ownership → P3 reuse path;
`verify-commit` / `staleness` → App/CI seams) or owned elsewhere (`steering-queue`, all model
selection → weave/atc) — none buildable today. Per the loop's method (and the standing goal), an
empty backlog **triggers a fresh deep-research sweep** of the four targets (kclaw0 + Archon +
attractor/automaton) to surface the *next* batch of runner-plane primitives. See the cycle-16 sweep
below for the refilled backlog.

### Cycle-16 deep-research sweep (backlog refill — 4 parallel agents, one per target)
A fresh, deeper sweep of all four targets *past* the cycle-1→16 applied set. Each agent deep-read one
source for runner-plane primitives not yet adopted; results cross-referenced for **convergence** (the
same mechanism named independently by ≥2 sources — the signal it's real, not one repo's quirk). Ranked
by convergence × buildable-now × non-redundancy. **Backlog is refilled.**

**Tier 1 — convergent + buildable-now (top picks):**
- ✓ **FATAL-first error taxonomy** (Archon `executor-shared.ts::classifyError`; convergent with kclaw0 +
  attractor retry loops) → **APPLIED (cycle 17, `feat/dispatch-error-taxonomy`).** Shipped as pure
  `recovery::classify_kernel_error` (FATAL-before-TRANSIENT precedence) + `FailureKind::KernelFatal`
  (escalate immediately) + `Outcome::KernelFatal` (fixed-enum telemetry class). See *Applied* row 17.
- ✓ **Dispatch provenance / authority gate** (automaton `policy-engine.ts::deriveAuthorityLevel`;
  convergent with attractor access-broker + Archon human-vs-agent origin) → **APPLIED (cycle 18,
  local)**. Shipped as an optional envelope `submitter { id, tier }` seam plus `AuthorityPolicy`
  route floors (`FXRUN_AUTHORITY_RULES=cycle=maintainer,agent=owner`); denials fail closed as
  `Outcome::AuthorityDenied` / `FailureKind::AuthorityDenied` before content inspection and never
  touch a kernel. See *Applied* row 18.
- ✓ **Delegation-target allowlist** (automaton `createX402DomainAllowlistRule`, fail-closed empty=deny;
  convergent with attractor egress allowlist) → **APPLIED (cycle 19, local)**. Shipped as
  `FXRUN_KERNEL_ALLOWLIST` over `loop`/`atc`/`hf`/`weave`, where unset is inert, set-empty denies all,
  and named kernels are the only reachable endpoints. Denials fail closed as `Outcome::TargetDenied` /
  `FailureKind::TargetDenied` before rate/breaker/budget/kernel invocation. See *Applied* row 19.
- ✓/⛔ **Max-in-flight concurrency cap + per-target single-flight mutex** (Archon `getActiveWorkflowRunByPath`
  older-wins lock + `ConversationLockManager`; convergent with kclaw0 `docker-exec` `maxContainers`) →
  **PARTIALLY APPLIED (cycle 20, local)**. The buildable per-target mutex seam is shipped as
  `SingleFlight` with normalized repo targets and deterministic older-wins denial/release semantics.
  The global max-in-flight cap remains blocked until the dispatcher serves concurrently; with today's
  one-connection accept loop it would be inert. See *Applied* row 20.

**Tier 2 — buildable-now, secondary:**
- ✓ **Idle / liveness watchdog** (Archon `idle-timeout.ts` — resets on each yielded message; "deadlock
  detector, not a work limiter"; convergent with kclaw0 `docker-exec`) → **APPLIED (cycle 21,
  local)**. `FXRUN_IDLE_TIMEOUT_SECS` / envelope `idle_timeout_secs` now bound kernel silence; the
  subprocess invoker refreshes liveness from stdout/stderr bytes and kills silent children as
  `Outcome::IdleTimeout` / `FailureKind::IdleTimeout`. See *Applied* row 21.
- ✓ **Deterministic route-selection contract** (attractor `select_edge` 5-step total order + `weight`) →
  **APPLIED (cycle 22, local)**. `RouteCandidate` selection is now `weight DESC, route_id ASC`, and
  each `KernelPlan` carries a route witness (`route_id`, `route_weight`) that the dispatch audit detail
  records on clean delegation. See *Applied* row 22.
- ✓ **State-gated route admission** (automaton `idle-only-tools.ts` × `AgentState`) →
  **APPLIED (cycle 23, PR #31).** `FXRUN_STATE_GATE=agent=full,cycle=conserving` maps route classes to
  the worst allowed `SurvivalTier`; degraded classes are deferred before single-flight, rate, breaker,
  budget, or kernel execution and return an attempt-0 retry-after directive (`Outcome::StateDeferred`).
- ▷ **Intra-job fan-out / amplification cap** (automaton `MAX_TOOL_CALLS_PER_TURN` + per-class per-turn
  cap). Bound how many sub-dispatches a single admitted job may emit — the gap the windowed rate-limit
  (#14) can't close (a burst *inside* one window). Inert until a job can emit sub-dispatches (P3).
  **Queued (P3-gated).**

**Tier 3 — seam-first / structural (later):**
- ▷ **Pre-dispatch outcome simulation** (kclaw0 `path-simulator.js::OutcomePredictor` — ex-ante
  `P(success)` + projected cost forecast; convergent with automaton lookahead). The *prospective* dual
  of the applied (retrospective) risk score (#16): forecast a path's terminal state + cost before
  delegating, admit/defer/reject against a threshold. **Seam-first.**
- ▷ **Holdout output-coverage gate** (kclaw0 `dark-factory.js::validateHoldout` — keyword coverage of
  the *request* in the *result*). **Output** admission (vs every applied gate's input/process side): did
  the kernel's result address the JobSpec's intent? **Seam-first** (needs an intent/result-summary seam).
- ▷ **auto_status silence gate** (attractor §2.6 — kernel returns with no structured outcome → fail-closed
  unless opted into a synthesized default). The third return case (distinct from timeout and error).
  **Seam-first** (needs the status-presence contract).
- ▷ **Reversibility classification + pre-action checkpoint** (automaton `audit-log.ts` `reversible` flag +
  pre-mod git snapshot). Tag each dispatch reversible/irreversible in the audit stream + capture a
  restore-point ref, upgrading recovery from *reroute* to *reroute-or-rollback*; feeds the provenance
  gate (irreversible ⇒ higher authority). **Seam-first.**
- ▷ **Pre/post dispatch hook middleware with veto** (attractor §9.7 `tool_hooks` — exit-non-zero = skip).
  Generalize the applied content scan (#15) into a pluggable, ordered pre-hook chain + a symmetric
  *post*-dispatch hook (the applied set has no post-hook beyond the audit log). **Seam-first refactor.**
- ⊕ **Dispatch-lifecycle FSM** (kclaw0 `dark-factory.js::transitionState`; convergent with automaton's
  name). A formal per-job state graph (`admit → delegate → return → verify`, terminal `blocked`) that
  *sequences* the existing gates as transition guards. Structural unification, larger. **Backlog.**
- ⊕ **Compensating rollback on partial admission** (Archon `resolver.ts` two-phase create + best-effort
  compensating destroy, "never mask the original error"). Release already-acquired resources if a later
  admission step fails. **Seam-first** (only bites once admission acquires >1 resource).

**Blocked (gated on another component):**
- ⛔ **Safe-reclamation reference-count gate** (Archon `cleanup-service.ts::getRemovalBlocker` — refuse
  teardown while another job references the workspace / it has un-persisted output / its product isn't
  provably landed). The same **worktree-reuse precondition** as the already-blocked adoption-ownership
  item — both unblock together at the P3 tmpfs-worktree reuse path.

**Convergence summary (cross-source signals, strongest first):** FATAL-first taxonomy (3 sources) ·
provenance/authority gate (3) · target allowlist (2) · concurrency/single-flight cap (2) · idle
watchdog (2) · outcome simulation (2). These six are the highest-confidence net-new primitives.

**Net after the refill:** the backlog was refilled with **15 net-new candidates** (6 convergent).
**Cycle 17 applied FATAL-first taxonomy, cycle 18 applied dispatch provenance/authority, cycle 19
applied the delegation-target allowlist, cycle 20 applied the buildable per-target single-flight
mutex, cycle 21 applied the idle/liveness watchdog, cycle 22 applied deterministic route selection,
cycle 23 applied state-gated route admission (PR #31), and the current branch applied rate-limit
clock freshness.** The remaining slices are the open Tier-0 hardening tasks (registration token
non-argv path, docs drift guard), Tier-1 automation expansion, and the P3/result/lifecycle backlog
below.

## Cycle-23 deep code audit backlog (fresh tasks)

Deep code audit date: **2026-06-23**. Local verification at audit time: `cargo fmt --all -- --check`,
`cargo test --workspace` (238 passed), `cargo clippy --workspace --all-targets --all-features -- -D
warnings`, and `cargo audit --json` (no vulnerabilities). The audit surfaced concrete code/design gaps
that now supersede the stale "empty backlog" assumption. Each item below is a cycle-sized task: write
failing tests first, implement, run local gates, open/merge a PR, then update this ledger and
`docs/automation-and-user-story.md`.

### Tier 0 — must-fix security/correctness

1. **Signed full-envelope authority provenance** — APPLIED (current branch)
   - Gap: `DispatchRequest.submitter` is trusted by the authority gate but is not covered by the
     `spec_json` HMAC. Approval has its own HMAC; submitter currently does not.
   - Upgrade: add a versioned envelope MAC or submitter proof binding `spec_json`, job signature,
     submitter identity/tier, and trust-relevant envelope fields.
   - Applied: `DispatchRequest.envelope_signature` signs a versioned envelope material binding
     `spec_json`, the job signature, approval, submitter, deadline, and idle timeout;
     `fxrun-dispatch` requires it whenever `FXRUN_AUTHORITY_RULES` is active.
   - Acceptance: replaying a valid signed `spec_json` with a forged `owner` submitter fails; legacy
     frames remain accepted only when no authority floor is configured.

2. **UDS socket ownership and permission hardening** — APPLIED (current branch)
   - Gap: `serve()` removes an existing socket path and binds without proving safe parent ownership,
     file type, symlink absence, or owner-only permissions.
   - Upgrade: validate parent directory owner/mode, remove only stale sockets, reject non-socket path
     collisions, bind in a private runtime dir, and chmod the socket to owner-only.
   - Applied: `prepare_socket_path()` requires a private parent directory, refuses non-socket
     collisions/symlinks, removes only stale sockets, and `harden_bound_socket()` sets mode `0600`.
   - Acceptance: tests cover unsafe parent, non-socket collision, and expected mode.

3. **Fresh workspace acquisition by construction** — APPLIED (current branch)
   - Gap: `TempDirProvider` uses `/tmp/fxrun-ws-$PID-$jobid` plus `create_dir_all`, so residue or a
     pre-created path can be adopted despite the “fresh isolated workspace” claim.
   - Upgrade: create a unique nonce path with atomic `create_dir`/`tempfile` semantics; never adopt an
     existing path; audit the workspace id.
   - Applied: `TempDirProvider` now uses nonce paths plus atomic `create_dir`, never
     `create_dir_all` on a deterministic job path.
   - Acceptance: pre-created candidate path is refused or bypassed; repeated same-job acquisitions are
     distinct; teardown still proves zero residue.

4. **Rate-limit clock freshness** — APPLIED (current branch)
   - Gap: `now_secs` is sampled before blocking `accept()`, so the next request after idle can be
     evaluated with stale time.
   - Upgrade: sample monotonic time after accept/read, immediately before `handle_request()`.
   - Applied: the production serve loop now accepts the connection first, then samples the
     server-lifetime monotonic clock and passes that fresh timestamp into `serve_stream()` /
     `handle_request()`, preserving `runner-core` as clock-injected and avoiding stale idle-time
     cooldown/window decisions.
   - Acceptance: a cooldown/window can expire while the server is idle before the next connection.

5. **Actions runner artifact verification** — APPLIED (current branch)
   - Gap: `fxrun-actions install` downloads and extracts the upstream runner tarball without checksum
     or attestation verification.
   - Upgrade: verify GitHub-published SHA256 and/or artifact attestation before extraction.
   - Applied: `fxrun-actions install` accepts `--sha256`/`RUNNER_SHA256`, otherwise downloads the
     release `.sha256` asset, verifies SHA-256 before extraction, and fails closed on mismatch.
   - Acceptance: bad digest refuses before `tar`; latest-version install verifies automatically.

6. **Actions registration token non-argv path**
   - Gap: `config.sh --token <token>` exposes the short-lived registration token in process argv.
   - Upgrade: prefer stdin/env/token-file path if the upstream runner supports it; otherwise isolate,
     minimize lifetime, and emit an explicit audited fallback warning.
   - Acceptance: normal registration path has no token in argv; fallback is opt-in/visible.

7. **CI supply-chain gate** — APPLIED (current branch)
   - Gap: local `cargo audit` passes, but CI does not enforce advisory or dependency policy checks.
   - Upgrade: add CI jobs for `cargo audit` and, if policy is adopted, `cargo deny`.
   - Applied: CI now includes a `Cargo audit` job (`cargo audit --deny warnings`).
   - Acceptance: PR checks fail on vulnerable advisories or denied crates/licenses.

8. **Ledger/docs drift guard** — APPLIED (current branch)
   - Gap: state-gated route admission landed in PR #31 while the ledger still said “Queued”.
   - Upgrade: add a cycle checklist/check that every merged upgrade updates Applied/Backlog docs and
     automation diagrams.
   - Applied: `fxrun forge-loop docs-drift` checks exported applied features against the upgrade ledger
     and CI now runs it after tests.
   - Acceptance: stale queued entries for exported modules are caught before merge.

### Tier 1 — automation and orchestration expansion

9. **Concurrent serve + global max-in-flight cap** — introduce bounded concurrent serving and enforce
   `FXRUN_MAX_IN_FLIGHT` alongside per-target single-flight.
10. **Intra-job fan-out / amplification cap** — when kernels can emit child dispatches, bind children
    to a parent job id and cap per-job/per-route amplification.
11. **Rule-citation audit schema** — APPLIED (cycle 25)
    - Every policy refusal carries `denied_by={gate, rule_id}` for queryable desktop/CLI explanations.
12. **Freshness and required-check input seams** — accept App-signed `head_sha_is_tip` and
    `required_checks_green` facts, then gate stale/unverified work before delegation.

### Tier 2 — result, rollback, and lifecycle

13. **Structured kernel result/status contract** — require a status JSON beside `FXRUN_COST_FILE`;
    missing status fails closed unless the route explicitly opts into synthesized success.
14. **Holdout output-coverage gate** — compare request/intent fields to kernel result summary and hold
    or fail success-without-answer cases.
15. **Pre-dispatch outcome simulation** — forecast success probability and projected cost before
    delegation; optionally defer/reject above thresholds.
16. **Reversibility classification + pre-action checkpoint** — tag reversible/irreversible dispatches,
    create restore points, and require higher authority for irreversible work.
17. **Pre/post hook middleware with veto** — ordered hook chain around dispatch; pre-hook veto skips,
    post-hook classifies output without masking original errors.
18. **Dispatch lifecycle FSM and resume journal** — persist per-job state (`admitted -> delegated ->
    returned -> verified/blocked`) so crash recovery and desktop timelines are first-class.
19. **Safe reclamation / workspace reuse gates** — before deleting or adopting a workspace, prove
    ownership, no active refs, persisted output, and landed products.

See [`automation-and-user-story.md`](automation-and-user-story.md) for the component inventory, ASCII
flow diagrams, automation boundary map, full agent automation story, and user communication flow.

## P3 execution milestone — the kernel-spawn invoker is real (the seams above are now consumed)

Many *Applied* rows defined a seam "the P3 invoker will consume" (workspace teardown #10, the
deadline hard-kill #12, secret redaction #13, content-scan string interpolation #15). **P3 landed
(`feat/p3-kernel-execution`, TDD):** `runner-dispatch` now ships a `SubprocessInvoker` that spawns the
real kernel binary (`loop`/`atc`/`hf`/`weave`, resolvable via `FXRUN_KERNEL_CMD_*`) inside the job's
isolated workspace (cwd), hands it the JobSpec on stdin + handoff env vars, **enforces the deadline by
killing the child** at the wall-clock limit (the deadlock-prone watchdog thread is gone — the invoker
owns the child, so it owns the kill), and relays the cost report the kernel writes to `FXRUN_COST_FILE`
(`JobCost::from_report`, fail-open). **Secret injection** is the envctl relay-bearer:
`FXRUN_INJECT_SECRETS` names env secrets relayed into the kernel child and registered with the
redactor (#13) so they never leak. Off by default (`FXRUN_KERNEL_EXEC=1` opts in; the dry-run invoker
remains the behaviour-preserving default so the runner still routes+governs+audits with no kernels
installed). Backed by a TDD test suite: 7 `SubprocessInvoker` unit tests (stub kernel) + a 5-case
**e2e/smoke** suite (`tests/e2e.rs`) that drives the **real binary over a real UDS socket** (real
spawn, cost relay, deadline-kill, secret inject×redact, pre-exec auth). `fxrun doctor` now reports
`uds dispatch` / `kernel execution` / `secret injection` all **WIRED**. The runner stays delegate-only:
it spawns + bounds + reclaims + relays; the kernel owns *how* the work runs.

## Deferred / out of scope (model-router — weave owns)

- `llm-client.js`, `subagent-profiles.js`, model selection/routing, provider switching (`cc-switch`).
  The runner exposes the `agent` seam (PR #4); weave drives it.

## Method (per cycle)

1. **Research** — read one kclaw0 script/system; note the mechanism.
2. **Surface** — decide the runner-plane analogue (or mark out-of-scope → weave).
3. **Apply** — implement in `runner-core` (+ wire a binary), test, keep CI green, land a PR.
4. **Record** — move the item to *Applied*; add anything newly seen to *Surfaced*.
| 43 | **Codex deep target mining:** GitHub Action, permissions, subagents, awesome-codex-cli, oh-my-codex | **Additive advanced harness surfaces.** The forge-loop now has a source-attributed target-mining ledger, a permission-profile migration blueprint kept separate from active `sandbox_mode`, two additional read-only custom agents (`forge-loop-researcher`, `forge-loop-ci-sentinel`), PreToolUse/SubagentStart/SubagentStop hook witnesses, and a Codex Action structured output schema wired through `codex-args --output-schema`. `components-audit --strict` now requires these new surfaces. | `.codex/agents/*`, `.codex/hooks/*`, `.codex/permissions/forge-loop-workspace.toml`, `.github/codex/schemas/forge-loop-output.schema.json`, `.github/workflows/codex-forge-loop.yml`, `docs/forge-loop/codex-target-mining.md`, `runner-cli::forge_loop` (`components-audit`) | official Codex docs + ecosystem mining |
| 44 | **Codex target exhaustion audit:** source → application → guard | **Target-mining audit and full lifecycle hook witnesses.** `fxrun forge-loop target-mining-audit --strict` now proves each required target has source evidence, local application evidence, and regression guard evidence. CI runs the new guard. The `.codex` harness also covers PermissionRequest, PostToolUse, PreCompact, and PostCompact lifecycle witnesses so permissions posture, post-tool drift, and compaction continuity are machine-visible. | `.codex/hooks/forge_loop_permission_request.py`, `.codex/hooks/forge_loop_post_tool_use.py`, `.codex/hooks/forge_loop_compact_summary.py`, `.codex/hooks.json`, `docs/forge-loop/codex-target-exhaustion-matrix.md`, `runner-cli::forge_loop` (`target-mining-audit`) | official Codex docs + ecosystem mining |
| 46 | **Forge-loop cycle 02:** Codex Action parse guard | **Keep manual Codex workflow parseable.** The manual Codex Forge Loop workflow no longer uses `secrets.*` in a job-level `if`, avoiding GitHub workflow parse failures on push. A unit test guards against reintroducing that unsupported expression form while preserving the documented Codex Action controls. | `.github/workflows/codex-forge-loop.yml`, `runner-cli::forge_loop` (`codex_github_action_workflow_uses_documented_controls`) | local + GitHub Actions |
| 47 | **Forge-loop cycle 03:** worktree isolation contract | **Machine-checked cycle isolation.** The `.codex` harness now carries a worktree isolation contract requiring named disposable worktrees, merge-before-next-cycle sequencing, no shared mutating checkout, and evidence for branch/PR/SHA plus component and target-mining audits. The forge-loop prompt references the contract and tests/components-audit guard it. | `.codex/worktrees/forge-loop-isolation.toml`, `.codex/prompts/forge-loop.md`, `.github/codex/prompts/forge-loop.md`, `runner-cli::forge_loop` | local |
| 48 | **Forge-loop cycle 04:** hook manifest | **Lifecycle hook inventory.** The `.codex` hook surface now has a machine-readable manifest mapping each lifecycle event to its script and expected JSON key, making hook coverage reviewable without reverse-engineering `hooks.json`. Components-audit and unit tests guard the manifest. | `.codex/hooks/forge-loop-hooks.manifest.json`, `runner-cli::forge_loop` | local |
| 49 | **Forge-loop cycle 05:** cycle evidence checklist | **Completion evidence contract.** The `.codex` harness now has a per-cycle evidence checklist requiring strict-upgrade-only behavior, commit/push/PR, auto-merge, gate commands, merge timestamp, and main fast-forward proof before a cycle can be called done. Components-audit and unit tests guard the checklist and prompt reference. | `.codex/checklists/forge-loop-cycle.toml`, `.codex/prompts/forge-loop.md`, `.github/codex/prompts/forge-loop.md`, `runner-cli::forge_loop` | local |
| 50 | **Runner flow / kclaw0 target:** sustain local runners and prove PR flow | **Runner sustain workflow plus flow audit.** The harness now translates the kclaw0 Dark Factory/swarm target into a local runner-flow evidence contract, adds a scheduled/manual two-slot self-hosted sustain workflow that runs useful forge-loop audits, and adds `fxrun forge-loop runner-flow-audit` to report active/queued work, open PR pressure, required-check queues, idle-without-work, and seamless PR-flow evidence. | `.github/workflows/runner-sustain.yml`, `docs/forge-loop/kclaw0-runner-flow-target.md`, `runner-cli::forge_loop` (`runner-flow-audit`) | local + GitHub Actions |
| 51 | **Runner sustain bridge duration:** reduce idle gaps between scheduled runs | **Bridge-duration useful runner work.** Runner Sustain now runs every 10 minutes, keeps both local runner slots performing useful forge-loop audits for a bounded default of 14 minutes, emits tick evidence, and caps jobs at 20 minutes. This creates active/queued useful work across schedule boundaries while leaving the long 12+ hour kclaw0 persistence proof as an explicit observed-window requirement. | `.github/workflows/runner-sustain.yml`, `docs/forge-loop/kclaw0-runner-flow-target.md`, `runner-cli::forge_loop` (`runner_sustain_workflow_bridges_schedule_interval`) | local + GitHub Actions |
| 52 | **Runner black-factor window audit:** observed kclaw0 proof gate | **No completion claim without a 12h observed window.** `fxrun forge-loop runner-black-factor-audit` now parses GitHub run and PR history and only passes strict mode when the evidence shows at least a 12-hour observed window, 72 successful Runner Sustain runs, and clean merged PR flow. This converts the kclaw0 24/7 / 12+ hour persistence target into a falsifiable audit instead of a one-shot runner dispatch claim. | `runner-cli::forge_loop` (`runner-black-factor-audit`), `docs/forge-loop/kclaw0-runner-flow-target.md` | local + GitHub Actions history |
| 53 | **Runner sustain PR reserve:** keep PR flow ahead of filler work | **PR-aware runner sustain.** Runner Sustain now runs on a 5-minute cadence with one reserve-safe local lane, a bounded 5-minute default duration, a 10-minute timeout, read-only Actions/PR permissions, and an early-yield gate when open PRs have pending or failed local required checks. This keeps useful dark-factory work flowing without letting filler work starve PR merge checks. | `.github/workflows/runner-sustain.yml`, `docs/forge-loop/kclaw0-runner-flow-target.md`, `runner-cli::forge_loop` (`runner_sustain_workflow_bridges_schedule_interval`) | local + GitHub Actions |
| 54 | **Runner sustain overlap reserve:** remove post-sustain idle gaps | **Reserve-safe overlap with mid-run PR yield.** Runner Sustain keeps the one-lane PR reserve model but raises the default useful-work window to 6 minutes on a 5-minute cadence and re-checks PR-local pressure between audit ticks. That creates queued/active overlap without consuming both local runners, so `runner-flow-audit --strict` can stay green between scheduled runs while PR-local checks still preempt filler work. | `.github/workflows/runner-sustain.yml`, `docs/forge-loop/kclaw0-runner-flow-target.md`, `runner-cli::forge_loop` (`runner_sustain_workflow_bridges_schedule_interval`) | local + GitHub Actions |
| 55 | **Runner black-factor duration proof:** reject yielded/short sustain runs | **Duration-proven black-factor evidence.** `runner-black-factor-audit` now requires `updatedAt` run-history evidence and only counts successful Runner Sustain runs whose observed wall-clock duration meets the minimum useful-work threshold. Yielded, cancelled, missing-duration, or too-short runs are reported as short/unproven instead of inflating the 72-run proof target. | `runner-cli::forge_loop` (`runner-black-factor-audit`), `docs/forge-loop/kclaw0-runner-flow-target.md` | local + GitHub Actions history |
| 56 | **Runner black-factor watch:** refill sustain and archive evidence | **GitHub-hosted black-factor watchdog.** A scheduled/manual watch workflow captures run/PR history, dispatches Runner Sustain when no sustain work is active or queued, runs `runner-flow-audit --strict`, records black-factor progress, and uploads all evidence as artifacts. This drives the runner pool toward the 12-hour proof window without spending a local runner lane on the watcher itself. | `.github/workflows/runner-black-factor-watch.yml`, `runner-cli::forge_loop` (`components-audit`, watch workflow test), `docs/forge-loop/kclaw0-runner-flow-target.md` | GitHub Actions + local |
| 57 | **Runner black-factor rolling window:** prevent stale sustain proof inflation | **Latest-window sustain counting.** `runner-black-factor-audit` now counts duration-proven Runner Sustain runs only inside the latest observed 12-hour proof window, while reporting total duration-proven runs separately. The watch workflow captures up to 1000 runs so the 12-hour proof window remains available even when 5-minute watch/sustain cadence creates hundreds of Actions records. | `.github/workflows/runner-black-factor-watch.yml`, `runner-cli::forge_loop` (`runner-black-factor-audit`) | local + GitHub Actions history |
| 58 | **Runner sustain watch backlog:** keep replacement sustain queued | **Small backlog top-up.** Runner Black Factor Watch now maintains a clamped 1-4 active/queued Runner Sustain backlog, default 2, instead of dispatching only after the pool goes empty. This reduces idle gaps while avoiding local-runner flooding and preserving the one-lane sustain reserve model. | `.github/workflows/runner-black-factor-watch.yml`, `runner-cli::forge_loop` (`runner_black_factor_watch_refills_and_artifacts_evidence`) | GitHub Actions |
| 59 | **Runner black-factor progress projection:** quantify remaining proof work | **Machine-visible remaining-run projection.** `runner-black-factor-audit` now reports the remaining latest-window Runner Sustain run count and the lower-bound minutes to reach the 72-run sustain target based on the minimum useful-work duration. Watch artifacts now expose not just pass/fail, but the exact remaining proof gap. | `runner-cli::forge_loop` (`runner-black-factor-audit`), `docs/forge-loop/kclaw0-runner-flow-target.md` | local + GitHub Actions artifacts |
| 60 | **Runner sustain opportunistic dual lane:** accelerate duration-proven proof | **Two-lane sustain with fast PR yield.** Runner Sustain now opportunistically uses both local runner lanes and checks PR-local pressure every 30 seconds, so the 72-run black-factor proof accrues faster while PR checks still preempt filler work. Runner Black Factor Watch dispatches the faster-yield sustain jobs. | `.github/workflows/runner-sustain.yml`, `.github/workflows/runner-black-factor-watch.yml`, `runner-cli::forge_loop` (`runner_sustain_workflow_bridges_schedule_interval`) | local + GitHub Actions |
