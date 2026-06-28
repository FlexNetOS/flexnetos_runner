# Agentic system proof gate

Updated: 2026-06-28

The owner target is broader than keeping the self-hosted runners busy. The end state is an agentic
system that runs 24/7 and is always researching, evaluating, adapting, growing, and improving while
PRs continue to flow to merge. A completion claim therefore needs one strict gate that composes the
previous narrower proofs.

`fxrun forge-loop agentic-system-audit --strict` is that gate. It requires:

1. **Always researching** — `target-mining-audit` must cover Codex docs, Codex ecosystem targets,
   `drdave-flexnetos/kclaw0`, and kclaw0 referenced resources.
2. **Always evaluating** — `components-audit`, `docs-drift`, and the per-cycle evidence checklist
   must be present and green.
3. **Always adapting** — `docs/kclaw0-upgrade-ledger.md` must show applied self-upgrade evidence,
   including kclaw0 target-mining and referenced-resource proof rows.
4. **Always growing** — live GitHub run/PR history must prove green PR flow plus black-factor
   growth: duration-proven Runner Sustain work, clean merged PRs, operations burn-in, and no idle
   runner gap when useful work exists.
5. **Fleet truth** — local self-hosted runner lanes must be attributable and not silently occupied
   by external unhealthy work during the proof.
6. **Self-improvement dispatch** — `Agentic System Watch` must periodically prove this audit and run
   after `Runner Black Factor Watch` has had a chance to top up sustain. When no PR is open,
   no PR-local checks are under pressure, and no Codex run is already active, it dispatches
   `Codex Forge Loop` for the next strict-upgrade research/adapt/grow cycle. The Codex workflow
   uses `OPENAI_API_KEY` when that repo secret exists and otherwise falls back to the
   already-authenticated local ChatGPT/Codex subscription on the self-hosted runner. Each completed
   Codex run rehydrates sustain work, waits/retries for PR-local pressure to clear, wakes
   `Runner Black Factor Watch`, and wakes `Agentic System Watch` with `trigger_source=codex_completion`;
   `Runner Black Factor Watch` also wakes from the fully completed `Codex Forge Loop` workflow_run event
   so pressure that is still visible during the explicit dispatch gets a post-completion retry. The
   agentic watch waits briefly
   for the completed Codex run to leave the active run list, so a growth cycle cannot leave the runner
   lane idle until the next cron tick. The same open-PR and active-Codex guards prevent stacking the next
   growth run before the current cycle's PR has merged.

Strict live proof command:

```bash
gh run list --limit 3000 --json name,status,conclusion,createdAt,updatedAt,event,displayTitle,url > /tmp/flexnetos-runs-live.json
gh pr list --state open --limit 100 --json number,title,state,mergedAt,statusCheckRollup,url > /tmp/flexnetos-prs-live.json
gh pr list --state all --limit 200 --json number,title,state,mergedAt,statusCheckRollup,url > /tmp/flexnetos-prs-history.json
cargo run -q -p runner-cli -- forge-loop agentic-system-audit --strict --json \
  --runs-json /tmp/flexnetos-runs-live.json \
  --open-prs-json /tmp/flexnetos-prs-live.json \
  --prs-history-json /tmp/flexnetos-prs-history.json
```

Non-strict mode is allowed in CI or local preflight to expose static gaps without requiring live
GitHub history, but non-strict output is not completion proof.
