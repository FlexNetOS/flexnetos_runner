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
| 1 | `scripts/loop-detection.js` (+ `fingerprint.js`) — "4 identical tool calls → loop" + SHA-256 keying | **`LoopGuard` circuit breaker**: trip fail-closed when the same *semantic* job (SHA-256 of `JobKind`, excluding volatile id) recurs ≥`threshold` within a `window` of dispatches. Dispatcher consults it before routing; tunable via `FXRUN_LOOP_WINDOW`/`FXRUN_LOOP_THRESHOLD`; default 4-in-8. | `runner-core::loopguard`, wired in `runner-dispatch` | feat/loop-breaker |

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
