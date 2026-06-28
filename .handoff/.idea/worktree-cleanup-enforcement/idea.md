# Unrefusable Worktree Cleanup Enforcement

Created: 2026-06-27
Status: raw idea captured for planning/task promotion
Source attachment: `/home/drdave/Desktop/meta/.local/share/codex/attachments/01e00d2c-3851-4b6a-a170-5434a13c877a/pasted-text-1.txt`

## Requirement outline

The idea requires cleanup enforcement below the agent through layered controls:

1. GitHub delete-head-branch-on-merge for remote cleanup.
2. Managed Codex hooks for session start/stop/compact safety.
3. Envctl/Codex wrapper that runs the safe reaper before and after Codex.
4. OS/user timer or envctl daemon as a crash/abandonment backstop.
5. Meta fleet audit/report that preserves dirty or uncertain state and reaps only clean merged worktrees/branches.

## Raw pasted idea

```text
 We need outside-the-agent enforcement, not just “please
  remember.” The design should be layered so cleanup happens even if
  the model forgets, refuses, compacts, or crashes.

  I’d implement this in 5 layers:

  1. GitHub remote cleanup
      - Enable “delete head branch on merge” for every repo.
      - This cleans remote PR branches after merge.
      - It does not clean local worktrees, so it’s necessary but
        insufficient.

  2. Codex managed hooks
      - Use Codex SessionStart, Stop, PreCompact, and PostCompact
        hooks.

      - SessionStart: run safe reaper before work starts.
      - Stop: run safe reaper again and emit dirty-worktree report.
      - PreCompact: write a durable resume packet before compact.
      - PostCompact: verify the resume packet exists.
      - Important: project-local hooks can be reviewed/disabled, but
        managed hooks from system/MDM/cloud/requirements sources are
        policy-trusted and cannot be disabled from the hook browser.

  3. Envctl/Codex wrapper
      - Route all codex entrypoints through the meta/envctl-
        installed binary path.

      - Wrapper does:

        rtk bash "$META_ROOT/envctl/scripts/reap-worktrees.sh"
        --apply
        codex "$@"
        status=$?
        rtk bash "$META_ROOT/envctl/scripts/reap-worktrees.sh"
        --apply
        exit "$status"

      - This catches normal startup/exit even if Codex hooks fail.

  4. OS/user timer
      - Add a systemd --user timer or envctl daemon job:
          - every 10–30 minutes
          - at login
          - maybe after network reconnect

      - It runs the safe reaper and writes a report.
      - This catches abandoned sessions and crashed agents.

  5. Meta fleet audit/report
      - Fleet-wide command that classifies:
          - merged + clean local branch/worktree → reap
          - merged + dirty → preserve and report
          - open PR → preserve
          - no PR / no upstream → report
          - .handoff state → preserve

      - Dirty state should never be deleted automatically. It should
        be surfaced as “needs publish/preserve/label.”

  The key phrase: unrefusable cleanup must live below the agent.

  Recommended concrete architecture:

  /etc/codex/requirements.toml
          ↓
  managed Codex hooks cannot be disabled in-session
          ↓
  $META_ROOT/.local/bin/codex wrapper
          ↓
  envctl/scripts/reap-worktrees.sh --apply
          ↓
  systemd user timer as crash/abandonment backstop
          ↓
  GitHub delete-branch-on-merge for remote refs

  I would not rely on only .codex/hooks.json inside repos. That is
  helpful for parity, but not “cannot be refused.” The non-refusable
  version is:

  - system/managed Codex requirements,
  - root/user-owned wrapper installed by envctl,
  - scheduled host cleanup,
  - GitHub repo setting for remote branch deletion.

  And the invariant stays strict-upgrade safe:

  > Auto-delete only clean, merged, proven-reapable worktrees/
  > branches. Dirty worktrees are never destroyed; they are reported
  > and routed into a recovery queue
```
