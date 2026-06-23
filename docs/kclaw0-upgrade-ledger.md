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
| 6 | **prior-art:** `strongdm/attractor` VALIDATE phase (refuse on structural ERROR before real work) + `retry_target`/`fallback_retry_target`/`wait.human` edges (declared recovery, not ad-hoc loops) | **Structural lint + declarative recovery routing.** Two seams: (a) a **`lint`** gate right after authentication refuses a malformed JobSpec (empty/`owner/name`-invalid repo, blank `head_sha`, `pr_number == 0`, blank ids) *before* a kernel is touched — a late, opaque kernel failure becomes an early, precise rejection. (b) **`recovery`** turns each failed dispatch into *advice carried back to the orchestrator*: transient `KernelFailed` → **retry-with-exponential-backoff** up to `FXRUN_MAX_RETRIES` (per-fingerprint `RetryLedger`, cleared on clean delegation), then **escalate-to-human** (open a review PR); `LoopTripped` / `Malformed` → escalate immediately (un-retryable). Directive rides back on `DispatchResponse.recovery` + the audit log. Delegate-only: the runner *recommends*, weave/App acts (owns the timer + escalation PR). | `runner-core::lint` + `runner-core::recovery`, wired in `runner-dispatch` (+ `DispatchResponse`/`DispatchEvent` fields, `Outcome::Malformed`) | feat/recovery-routing |

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

Ranked by runner-plane fit. Each names the kclaw0 source and the in-scope runner analogue.

- **`verify-commit.js`** → a **verification/backpressure gate** the dispatcher requires before
  reporting a job successful (the "oracle" of the dark-factory; research §5 "immutable oracle").
  High fit — the runner is where a "merge only if verified" rule belongs.
- **`cost-tracker.js`** → **per-job cost/turn telemetry** surfaced on the `DispatchResponse` (cost
  is a first-class keep-rule in SICA; research §5). Medium fit — runner can *carry* the number atc
  reports without owning model billing.
- **`staleness.js`** → **stale-job detection** (a JobSpec whose `head_sha` is no longer the PR tip →
  refuse as stale). Medium fit — needs a freshness input from the App.
- **`steering-queue.js` / `followup-queue.js`** → operator **steering / follow-up** signals the
  runner honors between jobs. Lower fit — closer to weave/atc orchestration; watch for a runner seam.
- **`survival.js`** (credit tiers → cheaper models → halt) → a **budget kill-switch** at the dispatch
  boundary (refuse new work past a budget). Medium fit — pairs with cost-tracker; halting is
  runner-appropriate, model-downgrade is weave's.
- **`checkpoint.js`** → mostly covered by the `handoff` kernel; revisit only if a *job-level*
  checkpoint seam is missing.

### Prior-art batch (widened beyond kclaw0 to the sources it names — ranked, runner-plane lens)
From the phase-3 research sweep of kclaw0's named prior-art (cited in `meta/DARK-FACTORY-RESEARCH.md`):
- ~~**#2 Goal-gate + retry-target rollback routing**~~ → **APPLIED (cycle 6, `feat/recovery-routing`).**
  Shipped as `runner-core::lint` (structural VALIDATE gate) + `runner-core::recovery` (retry-with-backoff
  → escalate-to-human directive on `DispatchResponse`). See *Applied* row 6.
- **#3 Survival-tier debounced halt** (`automaton` 5-tier balance ladder + 60-min-at-$0 grace before
  `dead`) → extend the `Governor` with **graduated degradation** (full → throttled → distress-only →
  halt) and a **debounced floor** (require the budget-exhausted condition to persist a grace window
  before refusing all dispatch — avoids boundary thrash). Medium-high. Model-*downgrade* action stays
  weave's; the runner takes the tier→admission-state ladder + halt floor only.
- **#4 Human-approval gate** (`coleam00/Archon` `ApprovalNode`/`interactive:true`; attractor `wait.human`
  hexagon) → a `requires_approval` admission state between budget and route — hold + surface an approval
  request when a JobSpec is flagged interactive or trips a policy band; resolve on approve/reject. Pairs
  with fork-PR isolation. Medium.
- **#5 Isolation-cleanup-on-fail** (`Archon` "fail → delete the worktree, zero residue") → make total
  teardown of the isolated worktree a **guaranteed post-condition** of the fail/breaker path. Low —
  mostly already covered by fork-PR isolation; a refinement, not a new primitive.
- **#6 `policy_decisions` audit stream** (`automaton` separate `policy_decisions` table + git lineage) →
  a **distinct admission-decision stream** (separate from job-execution events) so guardrail-tampering is
  detectable by lineage. Low-medium — extends the cycle-3 audit log; pairs with the constitution gate (#5 applied).

**Out of scope (weave/atc/CI), confirmed by the sweep:** `pi-subagents` reviewer-loop + per-agent model
overrides (atc/weave); `Conway-Research/skills` progressive disclosure (context-engineering);
`Understand-Anything` / `mempalace` / `chroma` / `second-brain-starter` (memory/KG/vector — weave/atc).

## Deferred / out of scope (model-router — weave owns)

- `llm-client.js`, `subagent-profiles.js`, model selection/routing, provider switching (`cc-switch`).
  The runner exposes the `agent` seam (PR #4); weave drives it.

## Method (per cycle)

1. **Research** — read one kclaw0 script/system; note the mechanism.
2. **Surface** — decide the runner-plane analogue (or mark out-of-scope → weave).
3. **Apply** — implement in `runner-core` (+ wire a binary), test, keep CI green, land a PR.
4. **Record** — move the item to *Applied*; add anything newly seen to *Surfaced*.
