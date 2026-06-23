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
| 4 | `scripts/cost-tracker.js` + `dark-factory.js::enforceBudget` (caps `usedTokens` **and** `usedUsd`) | **`atc → runner` cost seam + cost-aware `Governor`**: `KernelInvoker::invoke` now returns the job's [`JobCost`] (tokens + micro-USD) that `atc` measured; `Governor` became multi-dimensional (jobs **/** tokens **/** USD caps) with `admit()` (pre-dispatch gate) + `charge()` (post-dispatch, from the report). Caps via `FXRUN_DISPATCH_BUDGET` / `FXRUN_TOKEN_BUDGET` / `FXRUN_USD_MICROS_BUDGET`. **Fail-open**: unmeasured jobs ([`JobCost::ZERO`], today's dry-run) charge nothing, so cost caps are inert until `atc` reports — defining the seam without needing `atc` yet. Cost also lands in the audit log. | `runner-core::cost` + `runner-core::governor`, wired in `runner-dispatch` | feat/cost-aware-governor |

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

## Deferred / out of scope (model-router — weave owns)

- `llm-client.js`, `subagent-profiles.js`, model selection/routing, provider switching (`cc-switch`).
  The runner exposes the `agent` seam (PR #4); weave drives it.

## Method (per cycle)

1. **Research** — read one kclaw0 script/system; note the mechanism.
2. **Surface** — decide the runner-plane analogue (or mark out-of-scope → weave).
3. **Apply** — implement in `runner-core` (+ wire a binary), test, keep CI green, land a PR.
4. **Record** — move the item to *Applied*; add anything newly seen to *Surfaced*.
