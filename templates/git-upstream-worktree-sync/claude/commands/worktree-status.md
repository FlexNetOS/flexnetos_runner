---
model: haiku
description: Check background worktree verification status
argument-hint: "<branch-name>"
---

# Worktree Status

Check the background verification log created by `/worktree --check-cmd`.

## Implementation

```bash
#!/usr/bin/env bash
set -euo pipefail

BRANCH_NAME="$ARGUMENTS"
if [[ -z "$BRANCH_NAME" ]]; then
  echo "Usage: /worktree-status <branch-name>"
  exit 1
fi

WORKTREE_NAME="$(printf '%s' "$BRANCH_NAME" | tr '/[:space:]' '--' | tr -cd 'A-Za-z0-9._-')"
LOG_FILE="/tmp/worktree-check-$WORKTREE_NAME.log"

if [[ ! -f "$LOG_FILE" ]]; then
  echo "No background check log found for: $BRANCH_NAME"
  ls -1 /tmp/worktree-check-*.log 2>/dev/null || true
  exit 1
fi

if grep -q '^PASSED' "$LOG_FILE"; then
  grep '^PASSED' "$LOG_FILE"
elif grep -q '^FAILED' "$LOG_FILE"; then
  grep '^FAILED' "$LOG_FILE"
  echo ""
  tail -40 "$LOG_FILE"
else
  echo "Check still running or log incomplete:"
  tail -20 "$LOG_FILE"
fi
```
