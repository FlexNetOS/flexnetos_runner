---
id: 019f2578-8eaf-7e51-9de5-97372352ee50
slug: tasks/runner-state-settle-automation
title: "Automate runner-state settle pipeline"
type: task
status: completed
priority: high
tags: [runner, state, _work, settle, automation, guardrails]
---


## Problem
`_work` is intentionally preserved, but live runner activity keeps the repo dirty with timestamp churn, cache churn, runner-home gitconfig duplication, and GitKB runtime workspaces. The fix must not hide `_work` in `.gitignore`; it must classify, normalize, compress, snapshot, and gate preserved runner state.

## Goal
Add automated runner-state commands that settle preserved runner state without pretending the state does not matter:

- `fxrun runner-state audit`
- `fxrun runner-state normalize`
- `fxrun runner-state settle`

The full path should support:

```bash
fxrun runner-state settle --slots all --compress-old-cache --commit --push-pr
```

## Required classification
Detect dirty preserved-state files as:

- `live-runner-state`: pipeline mapping timestamps, active worker dirs
- `cache-state`: kache, cargo, envctl
- `config-churn`: duplicated `safe.directory` entries in runner-home `.gitconfig`
- `gitkb-runtime`: `.kb/.cache`, `.kb/workspaces`
- `denied-sensitive`: `.runner`, credentials, tokens, service identity files
- `unclassified`: fail under strict gate

Denied files fail closed.

## Required normalization
- `_work/runner-home-*/.gitconfig`: atomically dedupe repeated `safe.directory` entries after jobs.
- `_work/actions-runner-*-work/_PipelineMapping/.../PipelineFolder.json`: preserve `lastRunOn` changes by reporting/snapshotting, not leaving unexplained dirty state.
- `_work/runner-home-*/.cache/kache/*`: classify as cache; keep hot files live and compression-eligible old files routed through cache automation.
- `_work/runner-home-*/.cargo/.global-cache`: classify as cache; snapshot current state if intentionally preserved.
- `_work/runner-home-*/.cache/envctl/*`: route old entries through `fxrun cache compress`.
- `.kb/.cache`, `.kb/workspaces`: clean after `git-kb status/fsck` or preserve through a generated manifest/bundle; do not leave random workspaces dirty.

## Idle/liveness contract
`fxrun runner-state settle --require-idle` refuses to mutate when active runner workers exist unless forced. It must check at least process state and open files when tooling exists:

```bash
pgrep -af 'Runner.Worker|Runner.Listener'
lsof +D _work
systemctl --user status ...
```

## Snapshot/commit contract
If `_work` is tracked, automation may commit intentional runner-state snapshots:

```bash
fxrun runner-state settle --commit --message "chore(runner-state): settle runner runtime snapshot"
```

Optional publish path:

```bash
fxrun runner-state settle --commit --push-pr --automerge
```

## Strict gate
`fxrun runner-state audit --strict` fails if:

- dirty `_work` files are unclassified
- denied runner identity files changed
- cache files are too large and uncompressed
- `safe.directory` entries are duplicated
- active runner files would be compressed
- no manifest exists for compressed/restored state

## Better rule
Preserved runner state must be either:

1. clean,
2. actively owned by a running worker,
3. compressed with a manifest,
4. normalized by a settle command, or
5. committed as an intentional runner-state snapshot.

## Acceptance criteria
- Red tests prove denied/sensitive runner files fail closed and duplicate `safe.directory` entries are detected.
- `audit`, `normalize`, and `settle` command help exists under `fxrun runner-state`.
- Audit JSON classifies the current dirty-state seams.
- Normalize dedupes runner-home `.gitconfig` fixtures atomically.
- Settle composes audit, normalize, optional cache compression, optional commit/push-pr flags, and idle gates.
- `fxrun runner-state audit --strict` is suitable as a local/CI gate.
- Docs describe the preserve-state rule and emergency behavior.
