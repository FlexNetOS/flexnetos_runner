# FlexNetOS Runner Templates

This directory stores reusable source templates that can be copied into peer
repos without making generated runtime state authoritative.

## Available Templates

| Template | Purpose |
|---|---|
| [`git-upstream-worktree-sync/`](git-upstream-worktree-sync/) | Isolated git worktree helper for syncing fork repos from an upstream remote without disturbing local checkout work. |

Templates should be repo-neutral, provenance-bearing, and safe by default. Any
template that mutates a repo should prefer a preview, isolated worktree, or
explicit opt-in flag before touching protected branches.
