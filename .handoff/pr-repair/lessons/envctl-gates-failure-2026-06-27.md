# PR Repair Lesson — envctl gates failure signature

Observed: 2026-06-27
Source: `.handoff/pr-repair/scans/2026-06-27-classifications.json`
ICM topic: `context-flexnetos_runner`

## Signature

Multiple envctl planning-loop PRs showed the same high-level CI pattern:

- `rustfmt`: success
- `Analyze`: success
- `clippy`: success
- `MSRV`: success
- `cargo audit`: success
- `test`: success
- `CodeQL`: success
- `gates`: failure

Observed examples:

- `FlexNetOS/envctl#284` — `plan(loop-prompt-hub): cycle 6 — prompt_hub (Front-Door intent STORE)`
- `FlexNetOS/envctl#281` — `plan(loop-grit): cycle 5 — grit (merge/lock substrate for the union)`

## Supervisor classification

`cross_pr_shared_failure` candidate until gates logs prove the failures are independent.

## Recommended next action

Do not assign separate repair workers blindly. First inspect the `gates` logs for both PRs and decide
whether to create one shared envctl gates incident lane or individual PR repair lanes.

## Worker prompt hint

```text
Start by comparing gates logs for envctl#284 and envctl#281. If the failing command/signature is the
same, open one shared incident/base-fix lane. If the signatures differ, split into individual repair
lanes.
```

## Prototype confirmation — 2026-06-27

The `.idea` → `.handoff` prototype inspected live GitHub state for both PRs and confirmed the
shared signature. `FlexNetOS/envctl#284` gates run `28281171897` / job `83797089887` and
`FlexNetOS/envctl#281` gates run `28280061204` / job `83793897460` both fail in
`bash ci/gates/loop-state.sh` with:

```text
LOOP-STATE GATE FAIL — .handoff/loop/plan/loop_state.md: cycle_budget is not a non-negative integer (got: '<missing>')
```

Decision: keep the classification as `cross_pr_shared_failure` and route the next action to one
shared envctl loop-state incident lane, not two separate PR workers.
