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
