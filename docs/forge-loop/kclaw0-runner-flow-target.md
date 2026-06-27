# KClaw0 runner flow target

Updated: 2026-06-27

The referenced kclaw0 target is not expressed as a literal `black factor` string. The relevant kclaw0 Dark Factory / swarm targets extracted from `drdave-flexnetos/kclaw0` are:

- 24/7 autonomous operation.
- 300-agent / 4000-step / 12+ hour persistence evidence.
- 120-agent swarm plan with 480+ tests and 100% pass rate.
- Dark Factory GitHub flow: issue/PR state machine, tests before merge, auto-merge after approval/green gates.

For `flexnetos_runner`, the strict local translation is:

1. Self-hosted runners must not be silently idle while mergeable work exists.
2. Queued required local checks must trigger runner-pressure diagnosis.
3. PR flow proof must include green required checks, auto-merge armed, merged timestamp, and fast-forwarded `main`.
4. Claims of exceeding the target require machine evidence from `fxrun forge-loop runner-flow-audit --strict`, not screenshots or intent.

The target is not considered complete if there are no active/queued runs and no sustained workflow proving useful runner occupancy.


## Bridge-duration sustain policy

`runner-sustain.yml` is intentionally longer than a smoke check, but it must not consume the whole local runner pool while pull-request checks wait. Scheduled runs now fire every 5 minutes, keep one reserve-safe local runner lane performing useful forge-loop audits for a bounded default of 6 minutes, and cap the job at 10 minutes. The other local lane remains available for PR checks; the sustain job exits both before work starts and between audit ticks when open PRs have pending or failed local required checks.

This still does not by itself prove the 12+ hour kclaw0 persistence target; that proof requires an observed window of repeated successful sustain runs and green PR flow over the full target interval. The 5-minute cadence plus 6-minute default duration creates queued/active overlap with more than the required 72 sustain opportunities over 12 hours while preserving seamless PR flow as the higher-priority invariant.

## Observed-window black-factor audit

`fxrun forge-loop runner-black-factor-audit --strict` is the proof gate for any claim that this repo exceeded the kclaw0 target. It requires:

- an observed run-history window of at least 12 hours,
- at least 72 successful `Runner Sustain` workflow runs in that window (10-minute cadence over 12 hours), and
- at least one merged PR with clean required local checks.

Until those conditions pass from GitHub run/PR history, the goal remains in-progress even if instantaneous `runner-flow-audit --strict` passes.
