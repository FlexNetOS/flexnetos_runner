#!/usr/bin/env bash
set -Eeuo pipefail

ROOT=${ROOT:-/home/drdave/Desktop/meta/flexnetos_runner}
STAMP=${STAMP:-$(date -u +%Y%m%dT%H%M%SZ)}
BATCH=${BATCH:-$ROOT/_work/forge-loop-batch/${STAMP}-resume-05-10}
WTROOT="$BATCH/worktrees"
CYCLES="$BATCH/cycles"
mkdir -p "$WTROOT" "$CYCLES"
LOG="$BATCH/harness.log"

log() {
  printf '[%s] %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$LOG"
}

pr_field() {
  local pr=$1 field=$2
  gh pr view "$pr" --json "$field" --jq ".$field"
}

find_pr_for_branch() {
  local branch=$1
  gh pr list --head "$branch" --state all --json number --jq '.[0].number // empty'
}

ensure_pr_title() {
  local pr=$1 cycle=$2
  local title
  title=$(gh pr view "$pr" --json title --jq .title)
  case "$title" in
    feat:*|fix:*|chore:*|docs:*|test:*|refactor:*|ci:*) ;;
    *) gh pr edit "$pr" --title "chore: forge loop cycle $cycle" ;;
  esac
}

wait_merged() {
  local pr=$1
  local poll state merge_state
  for poll in $(seq 1 120); do
    state=$(gh pr view "$pr" --json state --jq .state)
    merge_state=$(gh pr view "$pr" --json mergeStateStatus --jq .mergeStateStatus)
    log "PR #$pr state=$state mergeState=$merge_state poll=$poll"
    if [[ "$state" == "MERGED" ]]; then
      return 0
    fi
    if [[ "$merge_state" == "BEHIND" ]]; then
      gh pr update-branch "$pr" || true
    fi
    sleep 15
  done
  log "PR #$pr did not merge before timeout"
  return 1
}

publish_cycle() {
  local cycle=$1 branch=$2 wt=$3
  local ahead pr
  git -C "$wt" status --short --branch > "$BATCH/cycle-$cycle.status.txt"
  if [[ -n "$(git -C "$wt" status --porcelain)" ]]; then
    log "cycle $cycle has uncommitted changes; stopping for inspection"
    return 2
  fi
  ahead=$(git -C "$wt" rev-list --count "origin/main..HEAD")
  log "cycle $cycle branch ahead=$ahead"
  if [[ "$ahead" == "0" ]]; then
    log "cycle $cycle produced no commit; stopping"
    return 3
  fi
  git -C "$wt" push -u origin "$branch"
  pr=$(find_pr_for_branch "$branch")
  if [[ -z "$pr" ]]; then
    gh pr create \
      --head "$branch" \
      --base main \
      --title "chore: forge loop cycle $cycle" \
      --body-file "$BATCH/cycle-$cycle-pr-body.md"
    pr=$(find_pr_for_branch "$branch")
  fi
  printf '%s\n' "$pr" > "$BATCH/cycle-$cycle.pr"
  ensure_pr_title "$pr" "$cycle"
  gh pr merge "$pr" --auto --squash || true
  wait_merged "$pr"
}

cd "$ROOT"
log "resume batch $BATCH begin"
git switch main
git fetch origin
git merge --ff-only origin/main
test -z "$(git status --porcelain)"

for cycle in 05 06 07 08 09 10; do
  log "cycle $cycle begin"
  git switch main
  git fetch origin
  git merge --ff-only origin/main
  test -z "$(git status --porcelain)"

  branch="codex/forge-loop-cycle-${STAMP}-${cycle}"
  wt="$WTROOT/cycle-$cycle"
  git worktree add -b "$branch" "$wt" origin/main
  cat > "$BATCH/cycle-$cycle-pr-body.md" <<BODY
Automated isolated forge-loop cycle $cycle of the resumed 10-cycle objective.

Batch: \`$BATCH\`

Local evidence captured in:
- \`$BATCH/cycle-$cycle.stdout.log\`
- \`$BATCH/cycle-$cycle.stderr.log\`
- \`$BATCH/cycle-$cycle.status.txt\`
BODY

  set +e
  (
    cd "$wt"
    cargo run -q -p runner-cli -- forge-loop run \
      --goal "Resume the interrupted 10-cycle forge-loop objective: execute isolated cycle $cycle of 10. Select exactly one small, TDD-first strict-upgrade improvement for fxrun forge-loop or its reliability/accuracy/speed guardrails that is not already applied. Commit the change locally with a conventional commit. If publishing directly, use PR title 'chore: forge loop cycle $cycle' and enable auto-merge once green. Do not start another cycle."
  ) >"$BATCH/cycle-$cycle.stdout.log" 2>"$BATCH/cycle-$cycle.stderr.log"
  code=$?
  set -e
  log "cycle $cycle fxrun_exit=$code"
  git -C "$wt" status --short --branch | tee "$BATCH/cycle-$cycle.status.txt"
  if [[ "$code" != "0" ]]; then
    log "cycle $cycle returned non-zero; attempting publish only if committed clean branch exists"
  fi
  publish_cycle "$cycle" "$branch" "$wt"
  log "cycle $cycle complete"
done

log "resume batch complete"
