---
model: haiku
description: Preview or remove local worktrees whose branches are already merged
argument-hint: "[--dry-run] [--base main|master|develop]"
---

# Clean Worktrees

Preview or remove worktrees whose branches have already been merged into the
selected base branch.

## Usage

```bash
/clean-worktrees --dry-run
/clean-worktrees --base develop --dry-run
/clean-worktrees --base develop
```

## Implementation

```bash
#!/usr/bin/env bash
set -euo pipefail

DRY_RUN=false
BASE_REF=""
RAW_ARGS="$ARGUMENTS"

if [[ -n "$RAW_ARGS" ]]; then
  eval "set -- $RAW_ARGS"
else
  set --
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      DRY_RUN=true
      shift
      ;;
    --base)
      BASE_REF="$2"
      shift 2
      ;;
    *)
      echo "unknown argument: $1"
      exit 1
      ;;
  esac
done

if [[ -z "$BASE_REF" ]]; then
  for candidate in main master develop; do
    if git show-ref --verify --quiet "refs/heads/$candidate"; then
      BASE_REF="$candidate"
      break
    fi
  done
fi
[[ -n "$BASE_REF" ]] || { echo "Could not infer base branch; pass --base"; exit 1; }

git worktree prune

CURRENT_DIR="$(pwd)"
FOUND=false

while IFS= read -r line; do
  path="$(echo "$line" | awk '{print $1}')"
  branch="$(echo "$line" | grep -oE '\[.*\]' | tr -d '[]' || true)"
  [[ -z "$branch" ]] && continue
  [[ "$branch" == "$BASE_REF" || "$path" == "$CURRENT_DIR" ]] && continue

  if git branch --merged "$BASE_REF" | grep -q "^[* ] $branch$"; then
    FOUND=true
    if [[ "$DRY_RUN" == true ]]; then
      echo "would remove: $branch at $path"
    else
      echo "removing: $branch at $path"
      git worktree remove "$path"
      git branch -d "$branch" 2>/dev/null || true
    fi
  fi
done < <(git worktree list)

if [[ "$FOUND" == false ]]; then
  echo "No merged worktrees found for base: $BASE_REF"
fi
```
