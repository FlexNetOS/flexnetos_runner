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

`runner-sustain.yml` is intentionally longer than a smoke check, but it must not hold the local runner pool when pull-request checks wait. Scheduled runs now fire every 5 minutes, the watch workflow can launch up to four active/queued one-lane `Runner Sustain` workflow runs, and each completed sustain run can dispatch one self-refill replacement when no PR-local pressure exists, with per-run concurrency groups so queued replacement proof runs are not cancelled by duplicate lane labels; the self-hosted runner pool still admits only the available local lanes, and each run performs bounded 5-minute useful forge-loop audits with a 10-minute job cap. Each run checks PR-local pressure before work starts, every 30 seconds between audit ticks, and again before self-refill, yielding quickly when open PRs have pending or failed local required checks; if the GitHub PR-pressure query itself fails, sustain treats that as pressure and exits successfully instead of letting filler-work network flakes poison the proof lane.

This still does not by itself prove the 12+ hour kclaw0 persistence target; that proof requires an observed window of repeated successful sustain runs and green PR flow over the full target interval. The 5-minute cadence plus a four-slot backlog creates continuous duration-proven workflow-run opportunities to reach the required 72-run proof over 12 hours while preserving seamless PR flow as the higher-priority invariant; the watch skips new top-ups when PR-local checks already need runner capacity.

## Runner Black Factor Watch and refill policy

`runner-black-factor-watch.yml` runs from GitHub-hosted capacity so it does not consume the local self-hosted runner pool. It captures run and PR history, writes a `runner-pressure.env` witness, tops up a small `Runner Sustain` active/queued backlog, proves instantaneous `runner-flow-audit --strict` when there is no PR-local pressure, records a non-strict runner-flow audit while pending PR checks temporarily own the runner lane, records non-strict black-factor progress, and uploads the run/PR/audit files as evidence artifacts. Failed PR-local checks still make the strict proof fail; pending PR checks make the watch yield green instead of adding red noise while the PR flow is actively draining. The backlog target is clamped to 1-4 runs and defaults to 4, meaning one active plus one queued replacement per local lane when no PR-local checks are waiting.

## Observed-window black-factor audit

`fxrun forge-loop runner-black-factor-audit --strict` is the proof gate for any claim that this repo exceeded the kclaw0 target. It requires:

- an observed run-history window of at least 12 hours,
- at least 72 duration-proven successful `Runner Sustain` workflow runs in the latest 12-hour proof window, where each counted run has both `createdAt` and `updatedAt` GitHub timestamps and ran for at least 5 minutes, and
- at least one merged PR with clean required local checks.

The run-history input must come from `gh run list --limit 1000 --json name,status,conclusion,createdAt,updatedAt,event,url`; a yielded, too-short, or old-outside-the-latest-proof-window Runner Sustain run is not counted as useful black-factor work. Until those conditions pass from GitHub run/PR history, the goal remains in-progress even if instantaneous `runner-flow-audit --strict` passes.

The audit also reports `remaining_sustain_runs` and `min_minutes_to_sustain_target`, a lower-bound projection based on the configured minimum useful-work duration. This keeps the gap to the 72-run proof target machine-visible on every watch artifact.
