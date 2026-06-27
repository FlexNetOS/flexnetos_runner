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

`runner-sustain.yml` is intentionally longer than a smoke check. Scheduled runs fire every 10 minutes and each local runner slot performs useful forge-loop audits for a bounded default of 14 minutes. This creates queued or active useful work across schedule boundaries, while a 20-minute job timeout prevents unbounded occupation.

This still does not by itself prove the 12+ hour kclaw0 persistence target; that proof requires an observed window of repeated successful sustain runs and green PR flow over the full target interval.
