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

`runner-sustain.yml` is intentionally longer than a smoke check, but it must not hold the local runner pool when pull-request checks wait. Scheduled runs now fire every 5 minutes, and the watch workflow can launch two separate one-lane `Runner Sustain` workflow runs with lane-scoped concurrency groups so both local runner lanes can perform bounded 6-minute useful forge-loop audits at the same time, with a 10-minute job cap. Each run checks PR-local pressure before work starts and every 30 seconds between audit ticks, yielding quickly when open PRs have pending or failed local required checks.

This still does not by itself prove the 12+ hour kclaw0 persistence target; that proof requires an observed window of repeated successful sustain runs and green PR flow over the full target interval. The 5-minute cadence plus two separate opportunistic lane runs creates more than enough duration-proven workflow-run opportunities to reach the required 72-run proof over 12 hours while preserving seamless PR flow as the higher-priority invariant.

## Runner Black Factor Watch and refill policy

`runner-black-factor-watch.yml` runs from GitHub-hosted capacity so it does not consume the local self-hosted runner pool. It captures run and PR history, tops up a small `Runner Sustain` active/queued backlog, proves instantaneous `runner-flow-audit --strict`, records non-strict black-factor progress, and uploads the run/PR/audit files as evidence artifacts. The backlog target is clamped to 1-4 runs and defaults to 2 so there is usually replacement sustain work waiting without flooding the local runner pool.

## Observed-window black-factor audit

`fxrun forge-loop runner-black-factor-audit --strict` is the proof gate for any claim that this repo exceeded the kclaw0 target. It requires:

- an observed run-history window of at least 12 hours,
- at least 72 duration-proven successful `Runner Sustain` workflow runs in the latest 12-hour proof window, where each counted run has both `createdAt` and `updatedAt` GitHub timestamps and ran for at least 5 minutes, and
- at least one merged PR with clean required local checks.

The run-history input must come from `gh run list --limit 1000 --json name,status,conclusion,createdAt,updatedAt,event,url`; a yielded, too-short, or old-outside-the-latest-proof-window Runner Sustain run is not counted as useful black-factor work. Until those conditions pass from GitHub run/PR history, the goal remains in-progress even if instantaneous `runner-flow-audit --strict` passes.

The audit also reports `remaining_sustain_runs` and `min_minutes_to_sustain_target`, a lower-bound projection based on the configured minimum useful-work duration. This keeps the gap to the 72-run proof target machine-visible on every watch artifact.
