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

`runner-sustain.yml` is intentionally longer than a smoke check, but it must not hold the local runner pool when required local checks wait. Scheduled runs now fire every 5 minutes, the watch workflow can launch up to four active/queued one-lane `Runner Sustain` workflow runs, and each completed sustain run can dispatch one self-refill replacement when no required local-check pressure exists, with per-run concurrency groups so queued replacement proof runs are not cancelled by duplicate lane labels; the self-hosted runner pool still admits only the available local lanes, and each run performs bounded 5-minute useful forge-loop audits with a 10-minute job cap. Each run checks open-PR local-check pressure and queued/in-progress `CI`/`Semantic PR Title` workflow-run pressure before work starts, every 30 seconds between audit ticks, and again before self-refill, yielding quickly when required local checks need the runner lane; if either GitHub pressure query fails, sustain treats that as pressure and exits successfully instead of letting filler-work network flakes poison the proof lane.

This still does not by itself prove the 12+ hour kclaw0 persistence target; that proof requires an observed window of repeated successful sustain runs and green PR flow over the full target interval. The 5-minute cadence plus a four-slot backlog creates continuous duration-proven workflow-run opportunities to reach the required 72-run proof over 12 hours while preserving seamless PR flow as the higher-priority invariant; the watch skips new top-ups when PR-local checks already need runner capacity.

## Runner Black Factor Watch and refill policy

`runner-black-factor-watch.yml` runs from GitHub-hosted capacity so it does not consume the local self-hosted runner pool. It is both schedule-driven and event-driven: every completed `CI` or `Semantic PR Title` workflow run wakes the watch through `workflow_run`, and every completed `Runner Sustain` run explicitly dispatches the watch with `trigger_source=sustain_completion` because GitHub suppresses some token-dispatched workflow-run chains. This keeps the sustain backlog rehydrated as soon as required-check pressure clears instead of waiting only for GitHub's cron scheduler. It captures run and PR history, writes a `runner-pressure.env` witness, tops up a small `Runner Sustain` active/queued backlog, proves instantaneous `runner-flow-audit --strict` when there is no local-check pressure, records a non-strict runner-flow audit while pending required checks temporarily own the runner lane, records non-strict black-factor progress, and uploads the run/PR/audit files as evidence artifacts. Failed required checks still make the strict proof fail after pending work drains; pending PR or main-branch local checks make the watch record a non-strict audit and stay green instead of adding red noise while required work is actively draining, even if the open PR still carries a stale failed-pressure witness from a superseded check run. The backlog target is clamped to 1-4 runs and defaults to 4, meaning one active plus one queued replacement per local lane when no required local checks are waiting.

## Observed-window black-factor audit

`fxrun forge-loop runner-black-factor-audit --strict` is the proof gate for any claim that this repo exceeded the kclaw0 target. It requires:

- an observed run-history window of at least 12 hours,
- at least 72 duration-proven successful `Runner Sustain` workflow runs in the latest 12-hour proof window, where each counted run has both `createdAt` and `updatedAt` GitHub timestamps and ran for at least 5 minutes, and
- at least one merged PR with clean required local checks.

The run-history input must come from `gh run list --limit 1000 --json name,status,conclusion,createdAt,updatedAt,event,url`; a yielded, too-short, or old-outside-the-latest-proof-window Runner Sustain run is not counted as useful black-factor work. Until those conditions pass from GitHub run/PR history, the goal remains in-progress even if instantaneous `runner-flow-audit --strict` passes.

The audit also reports `remaining_sustain_runs` and `min_minutes_to_sustain_target`, a lower-bound projection based on the configured minimum useful-work duration. This keeps the gap to the 72-run proof target machine-visible on every watch artifact.

## Operational SLO burn-in audit

`fxrun forge-loop runner-ops-slo-audit --strict` is the broader maturity gate for claims that the dark-factory runner operation is unattended, not merely threshold-complete. It consumes current GitHub run history and open-PR status, then checks a configurable burn-in window for bounded local-runner idle gaps across productive `Runner Sustain`, `CI`, and `Semantic PR Title` intervals, an active/queued sustain backlog at audit time, successful event-driven `Runner Black Factor Watch` rehydration after workflow completions, either from `workflow_run` or the explicit `sustain_completion` watch dispatch, zero unrecovered failed operational workflow runs inside the window, and seamless open-PR local checks. Superseded cancellations with a nearby successful replacement do not burn the failure budget; for named watch runs, a successful replacement in the same watch-family counts even when the trigger source in the run name differs. Unrecovered failures still do.

The default CLI window is intentionally short enough for rapid regression checks (`--min-window-hours 1`), while an operational completion claim should raise that window to the owner's burn-in target (for example 24 or 72 hours) and keep `--max-failed-ops-runs 0`. This closes the earlier ambiguity between "black-factor threshold exceeded" and "operations maturity complete": the black-factor audit proves the kclaw0 duration/count target, while the SLO audit proves that the current automation can keep itself rehydrated without visible idle gaps or failed ops over the selected burn-in window.

## Fleet lane ownership audit

The local self-hosted runners are a shared fleet resource, so a repo-local GitHub run list can look healthy while another repository is occupying one of the physical lanes. `fxrun forge-loop runner-fleet-audit --strict` closes that blind spot for operator-side proofs by scanning live GitHub Actions job environments that are still attached to a `Runner.Worker` process from procfs, deduplicating child processes into unique workflow jobs, and failing when any job outside the expected repository owns a local runner lane. This does not replace the run-history SLO; it explains delayed sessions and queued checks that are invisible to this repository's Actions history.

## End-to-end agentic system audit

`fxrun forge-loop agentic-system-audit --strict` composes the runner-flow, black-factor, operations
SLO, fleet-lane, component, docs-drift, and target-mining gates into one completion proof for the
owner's broader 24/7 agentic-system claim. It must be green before claiming the system is always
researching, evaluating, adapting, growing, and improving.

`agentic-system-watch.yml` runs on GitHub-hosted capacity every 30 minutes and after `Runner Black
Factor Watch` completions, so it evaluates growth after the sustain backlog has had a chance to
rehydrate. It captures run/PR history, runs `agentic-system-audit --strict`, refreshes once if the
proof is momentarily early, and then dispatches `Codex Forge Loop` only when the proof is green,
no PR is open, no PR-local checks need the pipeline, and no Codex Forge Loop run is already active.
The dispatched Codex workflow uses `OPENAI_API_KEY` when present, otherwise it runs the local Codex
CLI on a self-hosted runner with the already-authenticated ChatGPT/Codex subscription stored in
`CODEX_HOME`. This is the scheduled growth lane that keeps the system researching/adapting after the
runner black-factor lane is already healthy without stacking a new self-upgrade PR before the previous
PR has merged.
