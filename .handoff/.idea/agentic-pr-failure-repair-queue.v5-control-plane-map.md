# Agentic PR Failure Repair Queue v5 — Control Plane Map and TODO

Created: 2026-06-27
Status: brainstorm / next implementation map
Prior files:

- `.handoff/.idea/agentic-pr-failure-repair-queue.md`
- `.handoff/.idea/agentic-pr-failure-repair-queue.v3-meta-control-plane.md`
- `.handoff/.idea/agentic-pr-failure-repair-queue.v4-research-plan.md`
- `.handoff/.idea/meta-skill-loading-parity-map.md`

## Why this v5 exists

The previous session/thread hit context issues and locked out before the brainstorm could be turned
into the next durable `.idea` artifact. This file captures the completed work recovered from the
repo/thread, then adds a clearer control-plane map and TODO list.

`.idea` is intentionally the brainstorming/design area. It is not executable queue state, not the
repair ledger, and not the source of truth for active work. Its job is to hold the idea until the
implementation target is clear enough to become code, commands, or policy.

## Recovered completed work from this repo/thread

The runner/execution-plane foundation is no longer hypothetical. The following chunks landed in
`FlexNetOS/flexnetos_runner` and should be treated as completed substrate for the PR repair design:

| PR | Merge | What completed | Why it matters |
|---:|---|---|---|
| #33 | `123e1c3` | Default Actions runner registration to org scope | Stops repo-local runner scope drift; establishes FlexNetOS org runner as canonical. |
| #34 | `4dbafd8` | Runner install checksum fallback | Keeps runner installs strict-upgrade and supply-chain verified when release checksum lookup changes. |
| #35 | `10b7710` | Repo-local runner runtime paths | Moves persistent runtime under this repo's ignored `_work/`, not stray `/home/drdave/_work`. |
| #36 | `b91e3f8` | Org runner smoke workflow | Adds a committed workflow to prove runner identity and repo-local workspace paths. |
| #37 | `d9400de` | Live runner evaluation tool | Adds `scripts/eval-runners.sh` with proof artifacts, timing, journals, and API snapshots. |
| #38 | `e213f2a` | Rate cooldown clock freshness test | Dispatch policy hardening continued while runner work was draining. |
| #39 | `dac6bf3` | Org runner-group dispatch repair | Used envctl GitHub App auth to repair selected runner-group repo membership. |
| #40 | `6161d15` | Harden org runner repair script | Removed hardcoded `secretctl` path and made proof output safer/portable. |
| #41 | `e48caa6` | Relocate `.handoff/.idea` into repo | Moved the four brainstorm files into their correct repo-local home. |

Current proven state recovered from the thread:

- Two org-scoped runners exist and are online:
  - `fxrun-drdave-TRX50-AI-TOP-flexnetos-01`
  - `fxrun-drdave-TRX50-AI-TOP-flexnetos-02`
- Runner binaries were observed at `actions/runner` `2.335.1`.
- Local `.runner` configs point to `https://github.com/FlexNetOS`, not a repo URL.
- The Default org runner group covers `86/86` active FlexNetOS repos after repair.
- Both runner slots passed live smoke after the repair.
- Post-merge CI and CodeQL passed for PRs #39, #40, and #41.
- The misplaced `/home/drdave/.handoff` was removed after `.idea` was moved into this repo.

Implication: the next design layer should assume the execution plane is healthy enough to support a
repair queue. Future failures should be classified as PR/test/platform/orchestration failures unless
runner API, service state, or runner-group membership proves otherwise.

## My complete feedback / synthesis

### 1. `meta pr-repair` should be the contract, not the agent

The important primitive is not a specific Codex/Claude worker. The important primitive is a stable
control-plane contract that any worker can operate through:

```text
meta pr-repair scan
meta pr-repair classify FlexNetOS/meta#66
meta pr-repair lane create FlexNetOS/meta#66
meta pr-repair assign repair-meta-66 --agent codex
meta pr-repair watch
meta pr-repair merge-green
```

Codex, Claude, Weave, Grit, or a human can all use this contract. The contract owns semantics; agent
front doors are replaceable.

### 2. Keep global supervision separate from local repair

The supervisor must own fleet-wide truth and queue ordering. Repair workers must own only one lane.
This prevents context collapse and prevents one agent from mixing unrelated PRs.

```text
supervisor:
  - scan PR/check state
  - classify failure type
  - rerun stale checks
  - create/close repair lanes
  - assign exactly one worker per lane
  - watch CI
  - merge/automerge green PRs
  - store repeated signatures

worker:
  - enter one repair lane
  - inspect PR diff and CI logs
  - patch only PR scope
  - run narrow reproducer first
  - run required gates
  - commit/push/update PR
  - stop at green, superseded, or evidenced blocker

verifier:
  - enforce no unrelated diff
  - require CI/check policy
  - merge or arm automerge
  - record result
```

### 3. Use hybrid state, but one canonical owner per state type

Do not prematurely create a new queue database. The existing systems already cover different parts
of the state problem.

Recommended state ownership:

| State type | Canonical owner | Reason |
|---|---|---|
| Fleet graph | `.meta.yaml` / `meta project list --json` | Already canonical for workspace repos, tags, dependencies, and paths. |
| Active repair lane | `meta worktree` metadata | A lane is a worktree with TTL, PR URL, owner, and status. |
| Durable queue/audit | `.handoff/pr-repair` or handoff ledger | Handoff is the natural local workflow/audit surface; binary ledger stays gitignored if used. |
| Repeated failure memory | ICM | Persistent cross-session learning and failure signatures. |
| Code intelligence | GitKB / `.kb` | Symbol/file/dependency lookup for repair targeting. |
| Task graph / prioritization UX | Beads if validated | May be better for DAG/kanban/critical-path views after inspection. |
| External truth | GitHub PR/check state | Remote CI status, mergeability, reviews, and comments. |
| Execution health | FlexNetOS runner API + systemd + eval artifacts | Distinguishes infra failure from PR failure. |

This means v1 of the implementation can be simple: project graph from `meta`, lane state from
`meta worktree`, queue artifact in `.handoff/pr-repair`, and lessons in ICM. Beads/Grit can be added
after validation instead of blocking the first prototype.

### 4. Front-door parity is mandatory but not the substrate

The accepted parity direction remains correct:

> Make `meta` CLI/MCP the shared substrate, then generate and verify Claude and Codex front doors
> from one canonical source.

Until parity tooling exists, every repair ticket must include exact CLI fallback commands. MCP tools
are useful when exposed, but they cannot be mandatory in an agent prompt.

Minimum worker startup contract:

```bash
cd /home/drdave/Desktop/meta
rtk meta project list --json
rtk meta git status
rtk meta worktree list --json
```

### 5. Manual supervisor mode should come before automation

The next proof should not be a big orchestrator. It should be a manual supervisor run on 2-3 real
failed PRs using the intended contract by hand:

1. Scan open PRs and check state.
2. Classify stale vs real failures.
3. Create one repair lane for each real failure.
4. Assign a worker prompt with exact commands.
5. Watch CI and merge green PRs.
6. Store lessons.

Only after that should `meta pr-repair scan/classify/lane/watch` be implemented.

### 6. Concurrency must respect runner capacity

With two self-hosted runner slots, initial CI-producing repair concurrency should be conservative:

```yaml
max_active_ci_repair_lanes: 2
max_local_analysis_lanes: higher_if_no_ci_contention
```

Workers may analyze locally in parallel, but pushes that trigger CI should be watched and paced.
Repeated failures across multiple PRs should create one shared incident instead of wasting both
runners on duplicate failures.

### 7. Failure classification is the key quality gate

The classifier must distinguish:

```yaml
stale_queued:
  action: rerun once before repair
runner_infra:
  action: fix runner group/service/labels before assigning PR work
merge_conflict:
  action: update branch or regenerate
pr_test_regression:
  action: assign repair worker
platform_specific:
  action: route to platform lane
cross_pr_shared_failure:
  action: create incident/base-fix lane
policy_or_metadata_failure:
  action: patch workflow/policy metadata
superseded:
  action: close or replace
flaky:
  action: rerun with evidence and record signature
```

This classifier is the difference between an autonomous repair system and agents randomly debugging
noise.

### 8. Context exhaustion must be designed around

The prior session locked out due to context issues. The workflow must assume long-running repair
threads will compact, restart, or die.

Every lane therefore needs durable pickup state:

- PR URL and branch.
- Worktree path.
- Last CI check/run IDs.
- Current classification.
- Assigned agent/session if any.
- Last action taken.
- Next required command.
- Stop condition.
- Links to logs/artifacts.

Workers should update lane state before any long or risky operation and before handing off.

## Proposed v5 control-plane shape

```text
GitHub PR/check state
        |
        v
+-------------------------------+
| meta pr-repair supervisor     |
| - scan open PRs               |
| - classify checks             |
| - refresh stale runs          |
| - create/close lanes          |
| - assign workers              |
| - pace CI                     |
| - merge/automerge green PRs   |
+---------------+---------------+
                |
                v
+-------------------------------+        +-------------------------------+
| meta workspace substrate      |        | durable state/knowledge       |
| .meta.yaml                    |        | .handoff/pr-repair            |
| meta worktree metadata        | <----> | ICM failure signatures        |
| meta exec/git/project         |        | GitKB / .kb code intelligence |
| meta MCP when exposed         |        | Beads graph if validated      |
+---------------+---------------+        +-------------------------------+
                |
                v
+-------------------------------+
| one repair lane per PR        |
| repair-<project>-<pr>         |
| exact branch/worktree scope   |
| exact worker prompt           |
+---------------+---------------+
                |
                v
+-------------------------------+
| repair worker                 |
| - inspect logs/diff           |
| - patch scope only            |
| - validate                    |
| - commit/push/PR update       |
+---------------+---------------+
                |
                v
+-------------------------------+
| verifier / merge gate         |
| - no unrelated diff           |
| - required checks green       |
| - merge/automerge             |
| - record lesson               |
+-------------------------------+
```

## Candidate durable artifacts after brainstorm graduates

These are not all required immediately, but they define the likely destination:

```text
.handoff/pr-repair/
  queue.jsonl                  # durable queue/audit projection, if text state is selected
  incidents/
    <timestamp>-<signature>.md # shared failures across PRs
  agents/
    <repo>-<pr>-assignment.md  # generated worker prompt/handoff
  lessons/
    failure-signatures.yaml    # local projection of lessons; ICM is cross-session memory
```

If handoff ledger is selected as canonical queue state, `queue.jsonl` should become a generated or
exported view, not a second source of truth.

## Draft TODO list

### A. Complete the remaining research maps

- [x] Draft `.handoff/.idea/pr-repair-control-plane-map.md`.
  - Map supervisor responsibilities to `meta` commands and GitHub APIs.
  - Define lane lifecycle from scan to merge.
  - Include exact manual command sequence for one PR.
- [x] Draft `.handoff/.idea/pr-repair-state-decision.md`.
  - Compare `meta worktree` metadata, handoff ledger, `.handoff/pr-repair`, ICM, Beads, and GitHub.
  - Choose canonical owner per state type.
  - Decide whether text files are source of truth or generated views.
- [x] Draft `.handoff/.idea/meta-pr-repair-command-sketch.md`.
  - Sketch CLI verbs, JSON output shape, and dry-run behavior.
  - Include CLI fallback and MCP equivalent where applicable.
- [x] Inspect current `meta worktree` implementation for `--from-pr`, `--meta`, `--ttl`, and metadata persistence. Current command is `meta git worktree`; top-level `meta worktree` is not exposed in this checkout.
- [x] Inspect `handoff` task/ledger model and decide whether PR repair tickets should become handoff tasks. Decision: handoff is durable queue/audit candidate; v1 uses `.handoff/pr-repair` projection until exact ledger integration is mapped.
- [x] Inspect Beads (`beads_rust`, `beads_viewer`) to decide whether it should own queue graph/UX. Decision: candidate graph/UX only; not v1 canonical state.
- [x] Inspect Grit for multi-agent assignment maturity. Decision: candidate assignment/runtime layer only; lane metadata remains canonical for active PR repair ownership.
- [x] Inspect GitKB / `.kb` query paths for worker repair targeting. Decision: code-intelligence query layer, not queue state.

### B. Prove manual supervisor mode

- [x] Run a manual scan of open FlexNetOS PRs and check states. Captured in `.handoff/pr-repair/scans/2026-06-27-open-prs.json` and manual proof notes.
- [x] Classify at least 2-3 PRs into stale, real regression, platform-specific, superseded, or shared incident. Captured 10 live classifications in `.handoff/pr-repair/scans/2026-06-27-classifications.json`.
- [x] Rerun stale checks before assigning workers. Manual proof found no selected lane with confirmed stale queued checks; rule preserved in assignment prompt before editing.
- [x] Create one repair lane manually with existing `meta worktree` commands. Current command surface is `rtk meta git worktree`; created `repair-envctl-pr-267`.
- [x] Generate one worker prompt from the v3/v5 template. See `.handoff/pr-repair/agents/envctl-267-assignment.md`.
- [ ] Verify one repair cycle through commit, push, PR update, CI watch, and merge/automerge.
- [x] Record any repeated signature in ICM. Stored the repeated envctl `gates` failure signature from PRs #284/#281 and added `.handoff/pr-repair/lessons/envctl-gates-failure-2026-06-27.md`.

### C. Design the first `meta pr-repair` prototype

- [x] Define `meta pr-repair scan --json` output.
- [x] Define `meta pr-repair classify OWNER/REPO#PR --json` output.
- [x] Define `meta pr-repair lane create OWNER/REPO#PR --dry-run` behavior.
- [x] Define `meta pr-repair watch --json` behavior.
- [x] Define `meta pr-repair merge-green --dry-run` behavior.
- [x] Decide implementation home: thin script/prototype first, then meta subprocess plugin, then stable top-level command/front doors.
- [x] Require all mutating commands to support `--dry-run` where practical. Captured in command sketch common options and per-command behavior.

### D. Add agent-surface parity work

- [x] Decide canonical source for core `meta` skills/prompts. Decision: `claude-plugin/skills/*/SKILL.md` is canonical for core meta guidance; Codex-specific runtime wrappers stay in `.codex` templates.
- [x] Draft `meta init codex` / `meta sync codex-skills` design. Captured in `pr-repair-agent-surface-parity-plan.md`.
- [x] Add parity verifier concept for `.claude`, `.codex`, plugin cache, and `meta-mcp` availability. Captured as `meta parity check agent-surfaces --json` sketch.
- [x] Mirror future PR repair front doors for Claude and Codex from the same source. Target surfaces documented for Claude commands/skills and Codex prompts/agents.
- [x] Ensure every worker prompt includes `rtk meta` CLI fallback commands. Added mandatory worker prompt invariant.

### E. Operational safeguards

- [x] Enforce one worker per PR lane. Captured as supervisor/assignment invariant in control-plane map and command sketch.
- [x] Enforce no repair worker can mutate outside its lane unless supervisor reclassifies as shared incident. Captured in worker prompt contract.
- [x] Cap CI-producing active lanes to runner capacity, initially `2`. Captured in V5 and preserved as prototype invariant.
- [x] Require rerun-once for stale queued checks before debugging. Captured in classification table and manual flow.
- [x] Require verifier approval/green checks before merge. Captured in verifier/merge-green gates.
- [x] Store context checkpoints before long-running work to survive compaction or lockout. Captured as lane pickup-state requirement and worker prompt invariant.

## V5 bottom line

The path is now clear enough for the next artifact and then a prototype:

```text
Brainstorm/design:
  v5 control-plane map (this file)
  -> control-plane map
  -> state decision
  -> command sketch

Prototype:
  manual supervisor mode
  -> thin meta-native scan/classify/lane wrapper
  -> worker prompt generation
  -> watch/merge automation

Substrate:
  meta CLI/MCP + .meta.yaml + meta worktree + GitHub checks + FlexNetOS runners
  with handoff/ICM/GitKB/Beads/Grit assigned explicit roles after inspection
```

Do not build a parallel orchestrator. Build a thin, meta-native PR repair supervisor that turns the
healthy runner execution plane into an autonomous repair-and-merge workflow.
