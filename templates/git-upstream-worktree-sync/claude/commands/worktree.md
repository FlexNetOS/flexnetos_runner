---
model: haiku
description: Create an isolated git worktree for safe local development
argument-hint: "<branch-name> [--base <ref>] [--fast] [--check-cmd '<cmd>']"
---

# Git Worktree Setup

Create an isolated git worktree so local changes do not disturb the current
checkout.

## Usage

```bash
/worktree feature/new-filter
/worktree fix/typo --fast
/worktree chore/sync-upstream-0.42.4 --base develop --check-cmd 'cargo check'
```

## Implementation

Execute this script with arguments from `$ARGUMENTS`:

```bash
#!/usr/bin/env bash
set -euo pipefail

RAW_ARGS="$ARGUMENTS"
BRANCH_NAME=""
BASE_REF=""
CHECK_CMD=""
SKIP_CHECK=false

if [[ -n "$RAW_ARGS" ]]; then
  eval "set -- $RAW_ARGS"
else
  set --
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --base)
      BASE_REF="$2"
      shift 2
      ;;
    --check-cmd)
      CHECK_CMD="$2"
      shift 2
      ;;
    --fast)
      SKIP_CHECK=true
      shift
      ;;
    *)
      if [[ -z "$BRANCH_NAME" ]]; then
        BRANCH_NAME="$1"
      else
        echo "unknown extra argument: $1"
        exit 1
      fi
      shift
      ;;
  esac
done

if [[ -z "$BRANCH_NAME" ]]; then
  echo "Usage: /worktree <branch-name> [--base <ref>] [--fast] [--check-cmd '<cmd>']"
  exit 1
fi

if [[ "$BRANCH_NAME" =~ [[:space:]\$\`] || "$BRANCH_NAME" =~ [~^:?*\\\[\]] ]]; then
  echo "Invalid branch name: $BRANCH_NAME"
  exit 1
fi

TOPLEVEL="$(git rev-parse --show-toplevel)"
COMMON_DIR="$(git rev-parse --git-common-dir)"
if [[ "$COMMON_DIR" != /* ]]; then
  COMMON_DIR="$(cd "$TOPLEVEL" && cd "$COMMON_DIR" && pwd)"
fi
REPO_ROOT="$(cd "$COMMON_DIR/.." && pwd)"

cd "$REPO_ROOT"

if [[ -z "$BASE_REF" ]]; then
  BASE_REF="$(git symbolic-ref --quiet --short HEAD 2>/dev/null || true)"
fi
[[ -n "$BASE_REF" ]] || { echo "Could not infer base ref; pass --base"; exit 1; }

WORKTREE_NAME="$(printf '%s' "$BRANCH_NAME" | tr '/[:space:]' '--' | tr -cd 'A-Za-z0-9._-')"
WORKTREE_DIR="$REPO_ROOT/.worktrees/$WORKTREE_NAME"
LOG_FILE="/tmp/worktree-check-$WORKTREE_NAME.log"

if ! grep -qE '^\.worktrees/?$' "$REPO_ROOT/.gitignore" 2>/dev/null; then
  mkdir -p "$COMMON_DIR/info"
  if ! grep -qE '^\.worktrees/?$' "$COMMON_DIR/info/exclude" 2>/dev/null; then
    printf '\n# FlexNetOS isolated git worktrees\n.worktrees/\n' >> "$COMMON_DIR/info/exclude"
  fi
fi

mkdir -p "$REPO_ROOT/.worktrees"
git worktree add "$WORKTREE_DIR" -b "$BRANCH_NAME" "$BASE_REF"

INCLUDE_FILE="$REPO_ROOT/.worktreeinclude"
if [[ -f "$INCLUDE_FILE" ]]; then
  while IFS= read -r entry || [[ -n "$entry" ]]; do
    [[ -z "$entry" || "$entry" =~ ^[[:space:]]*# ]] && continue
    entry="${entry#"${entry%%[![:space:]]*}"}"
    entry="${entry%"${entry##*[![:space:]]}"}"
    [[ -e "$REPO_ROOT/$entry" ]] || continue
    mkdir -p "$(dirname "$WORKTREE_DIR/$entry")"
    cp -R "$REPO_ROOT/$entry" "$WORKTREE_DIR/$entry"
  done < "$INCLUDE_FILE"
else
  cp "$REPO_ROOT"/.env* "$WORKTREE_DIR/" 2>/dev/null || true
fi

if [[ "$SKIP_CHECK" == false && -n "$CHECK_CMD" ]]; then
  (
    cd "$WORKTREE_DIR"
    echo "check started at $(date +%H:%M:%S)" > "$LOG_FILE"
    if bash -lc "$CHECK_CMD" >> "$LOG_FILE" 2>&1; then
      echo "PASSED at $(date +%H:%M:%S)" >> "$LOG_FILE"
    else
      echo "FAILED at $(date +%H:%M:%S)" >> "$LOG_FILE"
    fi
  ) &
  CHECK_STATUS="background check running: $LOG_FILE"
elif [[ "$SKIP_CHECK" == true ]]; then
  CHECK_STATUS="check skipped"
else
  CHECK_STATUS="no check command supplied"
fi

echo "Worktree ready: $WORKTREE_DIR"
echo "Branch: $BRANCH_NAME"
echo "$CHECK_STATUS"
echo ""
echo "Next:"
echo "  cd $WORKTREE_DIR"
```
