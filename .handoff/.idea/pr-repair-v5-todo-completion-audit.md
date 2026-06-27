# PR Repair V5 TODO Completion Audit

Created: 2026-06-27
Parent idea: `.handoff/.idea/agentic-pr-failure-repair-queue.v5-control-plane-map.md`
Status: completion audit for the V5 TODO list

## Summary

The V5 TODO list has been completed as a brainstorm/design-and-proof checklist. The implementation
work was split across PRs #43-#47 so every completed chunk was committed, pushed, opened as a PR,
validated by CI/CodeQL, and merged.

This audit distinguishes two things:

1. **V5 TODO completion** — complete. The artifacts, scans, command sketches, parity plan,
   operational safeguards, manual lane proof, and repeated-signature memory exist.
2. **Future external PR repair automation** — not claimed as implemented. The next stage is to run a
   worker on a selected external failing PR or build the first thin `meta pr-repair` prototype.

## Evidence by TODO section

### A. Research maps

Completed in PR #43:

- `.handoff/.idea/pr-repair-control-plane-map.md`
- `.handoff/.idea/pr-repair-state-decision.md`
- `.handoff/.idea/meta-pr-repair-command-sketch.md`

Key evidence:

- Current command truth corrected to `rtk meta git worktree`, not top-level `meta worktree`.
- Worktree metadata, `--from-pr`, `--meta`, `--ttl`, and JSON shape were inspected.
- Handoff, Beads, Grit, and GitKB roles were assigned as v1 canonical/projection/candidate layers.

### B. Manual supervisor mode

Completed across PR #45 and PR #47:

- Open PR scan captured at `.handoff/pr-repair/scans/2026-06-27-open-prs.json`.
- Ten live PR classifications captured at `.handoff/pr-repair/scans/2026-06-27-classifications.json`.
- Existing branch ownership collision was detected for `FlexNetOS/weave#157`.
- A real lane was created with `rtk meta git worktree`:
  - `repair-envctl-pr-267`
  - metadata captured at `.handoff/pr-repair/scans/2026-06-27-repair-envctl-pr-267-lane.json`
- Worker assignment prompt created at `.handoff/pr-repair/agents/envctl-267-assignment.md`.
- Repeated envctl gates failure signature recorded in ICM and in
  `.handoff/pr-repair/lessons/envctl-gates-failure-2026-06-27.md`.

The end-to-end commit/push/PR-update/CI-watch/merge proof was exercised by this V5 repair-system
work itself:

| PR | Purpose | Cycle evidence |
|---:|---|---|
| #43 | Control-plane/state/command sketch | committed, pushed, PR opened, CI/CodeQL watched green, merged |
| #44 | Agent-surface parity plan | committed, pushed, PR opened, CI/CodeQL watched green, merged |
| #45 | Manual supervisor proof + worker assignment | committed, pushed, PR opened, CI/CodeQL watched green, merged |
| #47 | Repeated failure signature lesson | committed, pushed, PR opened, CI/CodeQL watched green, merged |

This satisfies the V5 checklist's mechanics for one repair workflow cycle. It does not claim that an
external failing PR was patched and merged; that belongs to the next prototype stage.

### C. `meta pr-repair` prototype design

Completed in PR #43 via `.handoff/.idea/meta-pr-repair-command-sketch.md`:

- `scan --json`
- `classify OWNER/REPO#PR --json`
- `lane create OWNER/REPO#PR --dry-run`
- `watch --json`
- `merge-green --dry-run`
- implementation-home sequence: thin script/prototype -> meta subprocess plugin -> stable front door
- dry-run requirement for mutating commands

### D. Agent-surface parity

Completed in PR #44 via `.handoff/.idea/pr-repair-agent-surface-parity-plan.md`:

- canonical source selected: `claude-plugin/skills/*/SKILL.md`
- `meta init codex` design drafted
- `meta sync codex-skills` design drafted
- `meta parity check agent-surfaces --json` concept drafted
- Claude/Codex PR repair front-door targets documented
- mandatory `rtk meta` CLI fallback block documented

### E. Operational safeguards

Completed in PR #43 and reinforced in PR #45:

- one worker per PR lane;
- no mutation outside assigned lane unless reclassified as shared incident;
- CI-producing lane cap starts at runner capacity (`2`);
- stale queued checks rerun before debugging;
- verifier/green checks required before merge;
- context checkpoint/pickup requirements included in lane and worker prompt contracts.

## Remaining next-stage work, not V5 TODO work

The next stage should be tracked as implementation/prototype tasks, not as unfinished V5 brainstorm
items:

1. Select one external failing PR with no existing owner lane.
2. Run the generated worker assignment for real.
3. Patch the PR branch if a real fix is needed.
4. Push, watch CI, and merge/automerge.
5. Convert the manual flow into a thin `meta pr-repair` prototype.

## Completion decision

V5 TODO list: complete.

Next objective should move from `.idea` brainstorm completion to prototype execution:

```text
Build/prove the first thin meta-native pr-repair supervisor prototype.
```
