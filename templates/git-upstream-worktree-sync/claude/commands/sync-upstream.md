---
model: haiku
description: Sync a fork from its upstream remote in an isolated git worktree
argument-hint: "[--upstream upstream] [--upstream-branch main|master] [--base develop] [--branch chore/sync-upstream-...] [--check-cmd '<cmd>'] [--push]"
---

# Sync Upstream In Isolated Worktree

Create a separate git worktree, fetch the upstream remote, and merge the
upstream remote-tracking branch into a dedicated local sync branch. The current
checkout is not modified.

## Usage

```bash
/sync-upstream
/sync-upstream --upstream upstream --upstream-branch master --base develop
/sync-upstream --branch chore/sync-upstream-0.42.4 --check-cmd 'cargo test --workspace'
```

## Implementation

Execute this command from the repository root:

```bash
#!/usr/bin/env bash
set -euo pipefail

if [ ! -x scripts/sync-upstream-worktree.sh ]; then
  echo "Missing scripts/sync-upstream-worktree.sh"
  echo "Install it from the FlexNetOS template:"
  echo "  /home/flexnetos/FlexNetOS/src/flexnetos_runner/templates/git-upstream-worktree-sync/scripts/sync-upstream-worktree.sh"
  exit 1
fi

scripts/sync-upstream-worktree.sh $ARGUMENTS
```

## Notes

- The helper never resets, deletes, or force-pushes.
- `--push` only pushes the isolated sync branch to `origin`.
- Directly merging into `develop`, `main`, or `master` remains a human branch
  policy decision after inspection.
