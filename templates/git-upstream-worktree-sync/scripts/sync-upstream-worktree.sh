#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/sync-upstream-worktree.sh [options]

Options:
  --upstream <remote>          Upstream remote name. Default: upstream
  --upstream-branch <branch>   Upstream branch to merge. Default: remote HEAD, main, master, then develop
  --base <ref>                 Local base ref for sync branch. Default: current branch
  --branch <branch>            Sync branch name. Default: chore/sync-upstream-<branch>-<timestamp>
  --worktree-root <dir>        Worktree root. Default: .worktrees
  --check-cmd <command>        Run command after a clean merge, inside the worktree
  --push                       Push sync branch to origin after a clean merge
  -h, --help                   Show this help

Examples:
  scripts/sync-upstream-worktree.sh
  scripts/sync-upstream-worktree.sh --upstream upstream --upstream-branch master --base develop
  scripts/sync-upstream-worktree.sh --branch chore/sync-upstream-0.42.4 --check-cmd 'cargo test --workspace'
USAGE
}

die() {
  echo "error: $*" >&2
  exit 1
}

slugify_branch() {
  printf '%s' "$1" | tr '/[:space:]' '--' | tr -cd 'A-Za-z0-9._-'
}

absolute_path() {
  local path="$1"
  if [[ "$path" = /* ]]; then
    printf '%s\n' "$path"
  else
    printf '%s\n' "$(pwd)/$path"
  fi
}

UPSTREAM_REMOTE="upstream"
UPSTREAM_BRANCH=""
BASE_REF=""
SYNC_BRANCH=""
WORKTREE_ROOT=".worktrees"
CHECK_CMD=""
PUSH=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --upstream)
      [[ $# -ge 2 ]] || die "--upstream requires a value"
      UPSTREAM_REMOTE="$2"
      shift 2
      ;;
    --upstream-branch)
      [[ $# -ge 2 ]] || die "--upstream-branch requires a value"
      UPSTREAM_BRANCH="$2"
      shift 2
      ;;
    --base)
      [[ $# -ge 2 ]] || die "--base requires a value"
      BASE_REF="$2"
      shift 2
      ;;
    --branch)
      [[ $# -ge 2 ]] || die "--branch requires a value"
      SYNC_BRANCH="$2"
      shift 2
      ;;
    --worktree-root)
      [[ $# -ge 2 ]] || die "--worktree-root requires a value"
      WORKTREE_ROOT="$2"
      shift 2
      ;;
    --check-cmd)
      [[ $# -ge 2 ]] || die "--check-cmd requires a value"
      CHECK_CMD="$2"
      shift 2
      ;;
    --push)
      PUSH=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "not inside a git worktree"

CURRENT_TOPLEVEL="$(git rev-parse --show-toplevel)"
GIT_COMMON_DIR="$(git rev-parse --git-common-dir)"
if [[ "$GIT_COMMON_DIR" != /* ]]; then
  GIT_COMMON_DIR="$(cd "$CURRENT_TOPLEVEL" && cd "$GIT_COMMON_DIR" && pwd)"
fi
REPO_ROOT="$(cd "$GIT_COMMON_DIR/.." && pwd)"

cd "$REPO_ROOT"

git remote get-url "$UPSTREAM_REMOTE" >/dev/null 2>&1 || {
  echo "remote '$UPSTREAM_REMOTE' is not configured." >&2
  echo "add it with: git remote add $UPSTREAM_REMOTE https://github.com/OWNER/REPO.git" >&2
  exit 1
}

git remote get-url origin >/dev/null 2>&1 || die "origin remote is not configured"

echo "Fetching $UPSTREAM_REMOTE..."
git fetch --prune "$UPSTREAM_REMOTE"

if [[ -z "$UPSTREAM_BRANCH" ]]; then
  REMOTE_HEAD="$(git symbolic-ref --quiet --short "refs/remotes/$UPSTREAM_REMOTE/HEAD" 2>/dev/null || true)"
  if [[ -n "$REMOTE_HEAD" ]]; then
    UPSTREAM_BRANCH="${REMOTE_HEAD#"$UPSTREAM_REMOTE/"}"
  fi
fi

if [[ -z "$UPSTREAM_BRANCH" ]]; then
  for candidate in main master develop; do
    if git show-ref --verify --quiet "refs/remotes/$UPSTREAM_REMOTE/$candidate"; then
      UPSTREAM_BRANCH="$candidate"
      break
    fi
  done
fi

[[ -n "$UPSTREAM_BRANCH" ]] || die "could not infer upstream branch; pass --upstream-branch"

UPSTREAM_REF="$UPSTREAM_REMOTE/$UPSTREAM_BRANCH"
git rev-parse --verify "$UPSTREAM_REF^{commit}" >/dev/null || die "missing upstream ref: $UPSTREAM_REF"

if [[ -z "$BASE_REF" ]]; then
  BASE_REF="$(git symbolic-ref --quiet --short HEAD 2>/dev/null || true)"
fi
[[ -n "$BASE_REF" ]] || die "could not infer base ref from detached HEAD; pass --base"
git rev-parse --verify "$BASE_REF^{commit}" >/dev/null || die "missing base ref: $BASE_REF"

if [[ -z "$SYNC_BRANCH" ]]; then
  TS="$(date -u +%Y%m%dT%H%M%SZ)"
  SYNC_BRANCH="chore/sync-upstream-$UPSTREAM_BRANCH-$TS"
fi

[[ "$SYNC_BRANCH" != *" "* ]] || die "branch names with spaces are not supported"
[[ "$SYNC_BRANCH" != *".."* ]] || die "branch names with '..' are not supported"

WORKTREE_ROOT_ABS="$(absolute_path "$WORKTREE_ROOT")"
WORKTREE_NAME="$(slugify_branch "$SYNC_BRANCH")"
WORKTREE_DIR="$WORKTREE_ROOT_ABS/$WORKTREE_NAME"

if git show-ref --verify --quiet "refs/heads/$SYNC_BRANCH"; then
  die "local branch already exists: $SYNC_BRANCH"
fi
[[ ! -e "$WORKTREE_DIR" ]] || die "worktree path already exists: $WORKTREE_DIR"

if ! grep -qE '^\.worktrees/?$' "$REPO_ROOT/.gitignore" 2>/dev/null; then
  mkdir -p "$GIT_COMMON_DIR/info"
  if ! grep -qE '^\.worktrees/?$' "$GIT_COMMON_DIR/info/exclude" 2>/dev/null; then
    printf '\n# FlexNetOS isolated git worktrees\n.worktrees/\n' >> "$GIT_COMMON_DIR/info/exclude"
    echo "Added .worktrees/ to local git exclude."
  fi
fi

echo "Creating isolated sync worktree..."
mkdir -p "$WORKTREE_ROOT_ABS"
git worktree add "$WORKTREE_DIR" -b "$SYNC_BRANCH" "$BASE_REF"

INCLUDE_FILE="$REPO_ROOT/.worktreeinclude"
if [[ -f "$INCLUDE_FILE" ]]; then
  while IFS= read -r entry || [[ -n "$entry" ]]; do
    [[ -z "$entry" || "$entry" =~ ^[[:space:]]*# ]] && continue
    entry="${entry#"${entry%%[![:space:]]*}"}"
    entry="${entry%"${entry##*[![:space:]]}"}"
    [[ -z "$entry" ]] && continue
    SRC="$REPO_ROOT/$entry"
    [[ -e "$SRC" ]] || continue
    mkdir -p "$(dirname "$WORKTREE_DIR/$entry")"
    cp -R "$SRC" "$WORKTREE_DIR/$entry"
  done < "$INCLUDE_FILE"
else
  cp "$REPO_ROOT"/.env* "$WORKTREE_DIR/" 2>/dev/null || true
fi

echo "Merging $UPSTREAM_REF into $SYNC_BRANCH..."
set +e
(
  cd "$WORKTREE_DIR"
  git merge --no-edit "$UPSTREAM_REF"
)
MERGE_STATUS=$?
set -e

if [[ $MERGE_STATUS -ne 0 ]]; then
  echo ""
  echo "Upstream merge needs attention in:"
  echo "  $WORKTREE_DIR"
  echo ""
  echo "Conflict status:"
  git -C "$WORKTREE_DIR" status --short
  echo ""
  echo "After resolving:"
  echo "  cd $WORKTREE_DIR"
  echo "  git add <resolved-files>"
  echo "  git commit"
  exit "$MERGE_STATUS"
fi

if [[ -n "$CHECK_CMD" ]]; then
  echo "Running check command: $CHECK_CMD"
  (
    cd "$WORKTREE_DIR"
    bash -lc "$CHECK_CMD"
  )
fi

if [[ "$PUSH" == true ]]; then
  git -C "$WORKTREE_DIR" push -u origin "$SYNC_BRANCH"
fi

echo ""
echo "Sync worktree ready."
echo "  repo:          $REPO_ROOT"
echo "  worktree:      $WORKTREE_DIR"
echo "  base:          $BASE_REF"
echo "  sync branch:   $SYNC_BRANCH"
echo "  upstream ref:  $UPSTREAM_REF"
echo ""
echo "Inspect:"
echo "  git -C $WORKTREE_DIR status --short --branch"
echo "  git -C $WORKTREE_DIR log --oneline --decorate --max-count=8"
echo ""
echo "Optional direct land, only if branch policy allows:"
echo "  git switch $BASE_REF"
echo "  git merge $SYNC_BRANCH"
echo "  git push origin $BASE_REF"
