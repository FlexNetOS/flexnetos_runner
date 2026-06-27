#!/usr/bin/env bash
# Inspect and repair FlexNetOS org self-hosted runner group repository membership.
# Dry-run by default. Use --apply to add missing active repositories to the selected runner group.
set -euo pipefail

ORG="${FXRUN_ORG:-FlexNetOS}"
GROUP_ID="${FXRUN_RUNNER_GROUP_ID:-}"
OUT_ROOT="${FXRUN_ORG_RUNNER_OUT:-_work/org-runner-repair}"
META_ROOT="${META_ROOT:-}"
SECRETCTL="${FXRUN_SECRETCTL:-}"
INSTALLATION_ID="${FXRUN_GH_INSTALLATION_ID:-140063898}"
APPLY=0
INCLUDE_ARCHIVED=0

usage() {
  cat <<USAGE
Usage: $0 [--apply] [--org ORG] [--group-id ID] [--out DIR] [--include-archived]

Inspects GitHub org self-hosted runners and repairs selected runner-group repository access.
Uses envctl secretctl to mint a GitHub App token with organization_self_hosted_runners:write.
Default is dry-run; --apply performs missing repository additions.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --apply) APPLY=1; shift ;;
    --org) ORG="$2"; shift 2 ;;
    --group-id) GROUP_ID="$2"; shift 2 ;;
    --out) OUT_ROOT="$2"; shift 2 ;;
    --include-archived) INCLUDE_ARCHIVED=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1" >&2; usage >&2; exit 2 ;;
  esac
done

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing required command: $1" >&2; exit 127; }; }
need gh
need jq
need date

resolve_meta_root() {
  if [[ -n "$META_ROOT" ]]; then
    printf '%s\n' "$META_ROOT"
    return 0
  fi
  local dir="$PWD"
  while [[ "$dir" != "/" ]]; do
    if [[ -d "$dir/envctl" && -d "$dir/flexnetos_runner" ]]; then
      printf '%s\n' "$dir"
      return 0
    fi
    dir="$(dirname "$dir")"
  done
  return 1
}

resolve_secretctl() {
  if [[ -n "$SECRETCTL" ]]; then
    [[ -x "$SECRETCTL" ]] && { printf '%s\n' "$SECRETCTL"; return 0; }
    echo "FXRUN_SECRETCTL is set but not executable: $SECRETCTL" >&2
    return 127
  fi
  if command -v secretctl >/dev/null 2>&1; then
    command -v secretctl
    return 0
  fi
  local root=""
  root="$(resolve_meta_root || true)"
  if [[ -n "$root" ]]; then
    local candidates=(
      "$root/envctl/target/release/secretctl"
      "$root/envctl/target/debug/secretctl"
      "$root/usr/bin/secretctl"
      "$root/.local/bin/secretctl"
    )
    for candidate in "${candidates[@]}"; do
      [[ -x "$candidate" ]] && { printf '%s\n' "$candidate"; return 0; }
    done
  fi
  echo "missing executable secretctl; set FXRUN_SECRETCTL or META_ROOT" >&2
  return 127
}

SECRETCTL="$(resolve_secretctl)"

stamp="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$OUT_ROOT/$stamp"
mkdir -p "$OUT"

TOKEN="$($SECRETCTL mint-github \
  --installation-id "$INSTALLATION_ID" \
  --permissions organization_self_hosted_runners:write,metadata:read,actions:write \
  --ttl-secs 3600 \
  --output json | jq -r '.token')"
if [[ -z "$TOKEN" || "$TOKEN" == "null" ]]; then
  echo "secretctl did not return a GitHub token" >&2
  exit 1
fi

gh_app() { GH_TOKEN="$TOKEN" gh "$@"; }
api() { gh_app api "$@"; }

api "/orgs/$ORG/actions/runners" > "$OUT/org-runners-before.json"
api "/orgs/$ORG/actions/runner-groups" > "$OUT/runner-groups.json"

if [[ -z "$GROUP_ID" ]]; then
  GROUP_ID="$(jq -r '.runner_groups[] | select(.name == "Default") | .id' "$OUT/runner-groups.json" | head -1)"
fi
if [[ -z "$GROUP_ID" || "$GROUP_ID" == "null" ]]; then
  GROUP_ID="$(jq -r '.runner_groups[] | select(.visibility == "selected") | .id' "$OUT/runner-groups.json" | head -1)"
fi
if [[ -z "$GROUP_ID" || "$GROUP_ID" == "null" ]]; then
  echo "could not resolve runner group id; pass --group-id" >&2
  exit 1
fi

api --paginate "/orgs/$ORG/actions/runner-groups/$GROUP_ID/repositories" > "$OUT/group-repositories-pages.jsonl"
jq -s '{repositories: (map(.repositories // []) | add // [])}' "$OUT/group-repositories-pages.jsonl" > "$OUT/group-repositories-before.json"
api --paginate "/orgs/$ORG/repos?per_page=100&type=all" > "$OUT/org-repositories-pages.jsonl"
jq -s '{repositories: (map(. // []) | add // [])}' "$OUT/org-repositories-pages.jsonl" > "$OUT/org-repositories.json"

repo_filter='select(.archived == false)'
if [[ "$INCLUDE_ARCHIVED" == 1 ]]; then
  repo_filter='.'
fi
jq -r ".repositories[] | $repo_filter | [.id,.full_name] | @tsv" "$OUT/org-repositories.json" | sort -k2,2 > "$OUT/target-repositories.tsv"
jq -r '.repositories[] | [.id,.full_name] | @tsv' "$OUT/group-repositories-before.json" | sort -k2,2 > "$OUT/group-repositories-before.tsv"
awk -F'\t' 'NR==FNR {have[$1]=1; next} !have[$1] {print}' "$OUT/group-repositories-before.tsv" "$OUT/target-repositories.tsv" > "$OUT/missing-repositories.tsv"
missing_count="$(wc -l < "$OUT/missing-repositories.tsv" | tr -d ' ')"

group_summary="$(jq -r --arg id "$GROUP_ID" '.runner_groups[] | select((.id|tostring) == $id) | "id=\(.id) name=\(.name) visibility=\(.visibility) default=\(.default)"' "$OUT/runner-groups.json")"
{
  echo "# FlexNetOS Org Runner Group Repair — $stamp"
  echo
  echo "- org: \`$ORG\`"
  echo "- runner group: \`${group_summary:-$GROUP_ID}\`"
  echo "- mode: \`$([[ "$APPLY" == 1 ]] && echo apply || echo dry-run)\`"
  echo "- include archived: \`$INCLUDE_ARCHIVED\`"
  echo "- org runners: \`$(jq '.total_count' "$OUT/org-runners-before.json")\`"
  echo "- target repositories: \`$(wc -l < "$OUT/target-repositories.tsv" | tr -d ' ')\`"
  echo "- missing repositories: \`$missing_count\`"
  echo
  echo "## Missing repositories"
  echo
  if [[ "$missing_count" == 0 ]]; then
    echo "None."
  else
    awk -F'\t' '{print "- `" $2 "` (`" $1 "`)"}' "$OUT/missing-repositories.tsv"
  fi
} > "$OUT/summary.md"

if [[ "$APPLY" != 1 ]]; then
  echo "DRY-RUN: $missing_count repositories would be added to runner group $GROUP_ID. Evidence: $OUT"
  exit 0
fi

: > "$OUT/add-results.jsonl"
while IFS=$'\t' read -r repo_id full_name; do
  [[ -z "${repo_id:-}" ]] && continue
  echo "adding $full_name ($repo_id) to runner group $GROUP_ID"
  add_stdout="$OUT/add-$repo_id.json"
  add_stderr="$OUT/add-$repo_id.err"
  if api -X PUT "/orgs/$ORG/actions/runner-groups/$GROUP_ID/repositories/$repo_id" >"$add_stdout" 2>"$add_stderr"; then
    jq -cn --arg id "$repo_id" --arg full_name "$full_name" '{repo_id:$id,full_name:$full_name,ok:true}' >> "$OUT/add-results.jsonl"
  else
    err="$(cat "$add_stderr")"
    jq -cn --arg id "$repo_id" --arg full_name "$full_name" --arg error "$err" '{repo_id:$id,full_name:$full_name,ok:false,error:$error}' >> "$OUT/add-results.jsonl"
  fi
done < "$OUT/missing-repositories.tsv"

if jq -e 'select(.ok == false)' "$OUT/add-results.jsonl" >/dev/null; then
  echo "one or more repository additions failed; see $OUT/add-results.jsonl" >&2
  exit 1
fi

api --paginate "/orgs/$ORG/actions/runner-groups/$GROUP_ID/repositories" > "$OUT/group-repositories-after-pages.jsonl"
jq -s '{repositories: (map(.repositories // []) | add // [])}' "$OUT/group-repositories-after-pages.jsonl" > "$OUT/group-repositories-after.json"
jq -r '.repositories[] | [.id,.full_name] | @tsv' "$OUT/group-repositories-after.json" | sort -k2,2 > "$OUT/group-repositories-after.tsv"
awk -F'\t' 'NR==FNR {have[$1]=1; next} !have[$1] {print}' "$OUT/group-repositories-after.tsv" "$OUT/target-repositories.tsv" > "$OUT/missing-repositories-after.tsv"
after_missing="$(wc -l < "$OUT/missing-repositories-after.tsv" | tr -d ' ')"
api "/orgs/$ORG/actions/runners" > "$OUT/org-runners-after.json"
{
  echo
  echo "## Apply results"
  echo
  echo "- added repositories: \`$missing_count\`"
  echo "- missing after repair: \`$after_missing\`"
} >> "$OUT/summary.md"

if [[ "$after_missing" != 0 ]]; then
  echo "repair incomplete: $after_missing repositories still missing; see $OUT/missing-repositories-after.tsv" >&2
  exit 1
fi

echo "OK: runner group $GROUP_ID covers all target repositories. Evidence: $OUT"
