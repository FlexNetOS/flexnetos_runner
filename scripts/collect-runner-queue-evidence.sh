#!/usr/bin/env bash
# Collect active GitHub Actions job evidence across repos for runner-queue-audit.
set -euo pipefail

ORGS=("FlexNetOS")
REPOS=()
REPO_LIMIT="${FXRUN_QUEUE_REPO_LIMIT:-1000}"
RUN_LIMIT="${FXRUN_QUEUE_RUN_LIMIT:-20}"
OUT="${FXRUN_QUEUE_OUT:-_work/runner-queue/repo-jobs.json}"
RUN_AUDIT=0

usage() {
  cat <<USAGE
Usage: $0 [--org ORG]... [--repo OWNER/REPO]... [--repo-limit N] [--run-limit N] [--out FILE] [--audit]

Collects queued and in-progress workflow runs across every non-archived FlexNetOS org repo by
default, fetches their job labels/runner assignment, and writes the combined JSON accepted by:

  fxrun forge-loop runner-queue-audit --repo-jobs-json <FILE> --json

The audit separates shared local FlexNetOS runner-label pressure
(self-hosted, flexnetos, nix) from GitHub-hosted or vendor queues. Extra orgs/repos
can be added for triage with --org/--repo, but the default scope is the whole FlexNetOS org.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --org) ORGS+=("$2"); shift 2 ;;
    --repo) REPOS+=("$2"); shift 2 ;;
    --repo-limit) REPO_LIMIT="$2"; shift 2 ;;
    --run-limit) RUN_LIMIT="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --audit) RUN_AUDIT=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1" >&2; usage >&2; exit 2 ;;
  esac
done

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing required command: $1" >&2; exit 127; }; }
need gh
need jq
need mktemp
need sort

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
if [[ "${FXRUN_QUEUE_KEEP_TMP:-0}" == "1" ]]; then
  echo "keeping temp evidence in $TMP" >&2
else
  trap 'rm -rf "$TMP"' EXIT
fi

mkdir -p "$(dirname "$OUT")"

repos_file="$TMP/repos.txt"
: > "$repos_file"
for repo in "${REPOS[@]}"; do
  printf '%s\n' "$repo" >> "$repos_file"
done
if [[ "$REPO_LIMIT" =~ ^[0-9]+$ && "$REPO_LIMIT" -gt 0 ]]; then
  for org in "${ORGS[@]}"; do
    gh repo list "$org" --limit "$REPO_LIMIT" --json nameWithOwner,isArchived \
      --jq '.[] | select(.isArchived | not) | .nameWithOwner' >> "$repos_file" || true
  done
fi
sort -u "$repos_file" -o "$repos_file"

runs_jsonl="$TMP/runs.jsonl"
: > "$runs_jsonl"
while IFS= read -r repo; do
  [[ -z "$repo" ]] && continue
  for status in queued in_progress; do
    runs_json="$TMP/runs-${repo//\//_}-${status}.json"
    if ! gh run list --repo "$repo" --limit "$RUN_LIMIT" --status "$status" \
      --json databaseId,name,status,conclusion,event,displayTitle,headBranch,url \
      > "$runs_json" 2>/dev/null < /dev/null; then
      continue
    fi
    jq -c --arg repo "$repo" '.[] | {
          repository: $repo,
          run_id: (.databaseId | tostring),
          name: (.name // ""),
          run_status: (.status // ""),
          conclusion: (.conclusion // ""),
          event: (.event // ""),
          displayTitle: (.displayTitle // ""),
          headBranch: (.headBranch // ""),
          url: (.url // ""),
          jobs: []
        }' "$runs_json" >> "$runs_jsonl"
  done
done < "$repos_file"

deduped_runs="$TMP/runs-deduped.jsonl"
jq -s -c 'unique_by(.repository + ":" + .run_id)[]' "$runs_jsonl" > "$deduped_runs"

snapshots_jsonl="$TMP/snapshots.jsonl"
: > "$snapshots_jsonl"
while IFS= read -r run; do
  [[ -z "$run" ]] && continue
  repo="$(jq -r '.repository' <<< "$run")"
  run_id="$(jq -r '.run_id' <<< "$run")"
  jobs_json="$TMP/jobs-${repo//\//_}-${run_id}.json"
  if gh api --paginate "/repos/$repo/actions/runs/$run_id/jobs?per_page=100" \
    --jq '.jobs[] | {
      name: (.name // ""),
      status: (.status // ""),
      conclusion: (.conclusion // ""),
      runner_name: (.runner_name // ""),
      runner_group_name: (.runner_group_name // ""),
      labels: (.labels // []),
      html_url: (.html_url // "")
    }' 2>/dev/null < /dev/null | jq -s '.' > "$jobs_json"; then
    jq -n -c --argjson run "$run" --slurpfile jobs "$jobs_json" '$run + {jobs: $jobs[0]}' >> "$snapshots_jsonl"
  else
    jq -n -c --argjson run "$run" '$run + {jobs: []}' >> "$snapshots_jsonl"
  fi
done < "$deduped_runs"

jq -s '.' "$snapshots_jsonl" > "$OUT"
echo "wrote $OUT"

if [[ "$RUN_AUDIT" == "1" ]]; then
  # The meta parent may configure an experimental linker for CI speed. This collector is an
  # operator triage path, so prefer the system linker unless the caller explicitly set rustflags.
  export CARGO_ENCODED_RUSTFLAGS="${CARGO_ENCODED_RUSTFLAGS-}"
  if command -v rtk >/dev/null 2>&1; then
    (cd "$ROOT" && rtk cargo --config 'target.x86_64-unknown-linux-gnu.linker="cc"' run -q -p runner-cli -- forge-loop runner-queue-audit --repo-jobs-json "$OUT" --json)
  else
    (cd "$ROOT" && cargo --config 'target.x86_64-unknown-linux-gnu.linker="cc"' run -q -p runner-cli -- forge-loop runner-queue-audit --repo-jobs-json "$OUT" --json)
  fi
fi
