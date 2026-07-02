#!/usr/bin/env bash
# Evaluate FlexNetOS org self-hosted runners with live workflow_dispatch probes.
# Writes proof artifacts under _work/evals/<timestamp>/.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="${FXRUN_EVAL_REPO:-FlexNetOS/flexnetos_runner}"
WORKFLOW="${FXRUN_EVAL_WORKFLOW:-runner-smoke.yml}"
REF="${FXRUN_EVAL_REF:-main}"
POLL_SECS="${FXRUN_EVAL_POLL_SECS:-5}"
TIMEOUT_SECS="${FXRUN_EVAL_TIMEOUT_SECS:-300}"
OUT_ROOT="${FXRUN_EVAL_OUT:-$ROOT/_work/evals}"
ISOLATE=1
RUNNERS=(
  "01:fxrun-drdave-TRX50-AI-TOP-flexnetos-01:actions.runner.FlexNetOS.fxrun-drdave-TRX50-AI-TOP-flexnetos-01.service:$ROOT/_work/actions-runner-01-work:$ROOT/_work/repos/actions-runner-01"
  "02:fxrun-drdave-TRX50-AI-TOP-flexnetos-02:actions.runner.FlexNetOS.fxrun-drdave-TRX50-AI-TOP-flexnetos-02.service:$ROOT/_work/actions-runner-02-work:$ROOT/_work/repos/actions-runner-02"
)

usage() {
  cat <<USAGE
Usage: $0 [--no-isolate] [--repo OWNER/REPO] [--workflow FILE] [--ref REF] [--out DIR]
          [--poll-secs N] [--timeout-secs N]

Live-evaluates the two repo-local FlexNetOS org runners. Default behavior isolates each slot by
stopping its peer before dispatch, proving that the intended runner accepted and completed the job.

Artifacts:
  summary.md          human scorecard, failures, task output, and lessons learned
  metrics.jsonl       one JSON record per runner with granular turnaround timings and accuracy
  api-*.json          GitHub org runner API snapshots
  run-*.json/log      GitHub run metadata and raw job logs
  journal-*.log       local systemd journal excerpts
  diag-*.log          local runner diagnostic tails
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-isolate) ISOLATE=0; shift ;;
    --repo) REPO="$2"; shift 2 ;;
    --workflow) WORKFLOW="$2"; shift 2 ;;
    --ref) REF="$2"; shift 2 ;;
    --out) OUT_ROOT="$2"; shift 2 ;;
    --poll-secs) POLL_SECS="$2"; shift 2 ;;
    --timeout-secs) TIMEOUT_SECS="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1" >&2; usage >&2; exit 2 ;;
  esac
done

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing required command: $1" >&2; exit 127; }; }
need gh
need jq
need date
need systemctl
need journalctl
need sed
need awk

SECRETCTL="${FXRUN_EVAL_SECRETCTL:-}"
if [[ -z "$SECRETCTL" ]]; then
  for candidate in \
    "$ROOT/../envctl/target/debug/secretctl" \
    "$ROOT/../../envctl/target/debug/secretctl"
  do
    if [[ -x "$candidate" ]]; then
      SECRETCTL="$candidate"
      break
    fi
  done
fi
if [[ -z "$SECRETCTL" ]] && command -v secretctl >/dev/null 2>&1; then
  SECRETCTL="$(command -v secretctl)"
fi

if [[ -z "${GH_TOKEN:-}" && -n "$SECRETCTL" && -x "$SECRETCTL" ]]; then
  export GH_TOKEN="$($SECRETCTL mint-github --installation-id "${FXRUN_EVAL_GH_INSTALLATION_ID:-140063898}" --permissions organization_self_hosted_runners:write,metadata:read,actions:write --ttl-secs 3600 --output json | jq -r '.token')"
fi

iso_now() { date -u +%Y-%m-%dT%H:%M:%SZ; }
# GNU date's %3N is not portable in this environment; use epoch nanoseconds and divide.
now_ms() {
  local ns
  ns="$(date -u +%s%N)"
  echo $((10#$ns / 1000000))
}
epoch_ms() {
  local iso="$1" ns
  if [[ -z "$iso" || "$iso" == "null" || "$iso" == "0001-01-01T00:00:00Z" ]]; then
    echo 0
    return 0
  fi
  ns="$(date -u -d "$iso" +%s%N)"
  echo $((10#$ns / 1000000))
}
ms_delta() {
  local start="${1:-0}" end="${2:-0}"
  if (( start == 0 || end == 0 )); then echo 0; else echo $((end - start)); fi
}
ms_fmt() { awk -v ms="${1:-0}" 'BEGIN { printf "%.3fs", ms/1000 }'; }

stamp="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$OUT_ROOT/$stamp"
mkdir -p "$OUT"
SUMMARY="$OUT/summary.md"
METRICS="$OUT/metrics.jsonl"
: > "$METRICS"

{
  echo "# FlexNetOS Runner Evaluation — $stamp"
  echo
  echo "- repo: \`$REPO\`"
  echo "- workflow: \`$WORKFLOW\`"
  echo "- ref: \`$REF\`"
  echo "- isolate peers: \`$ISOLATE\`"
  echo "- started: \`$(iso_now)\`"
  echo
  echo "## Live Results"
  echo
  echo "| Slot | Runner | Conclusion | Accuracy | Dispatch→visible | Dispatch→created | Pickup latency | Exec time | Total | Run |"
  echo "|---|---|---:|---:|---:|---:|---:|---:|---:|---|"
} > "$SUMMARY"

api_snapshot() {
  local name="$1"
  gh api "/orgs/${REPO%%/*}/actions/runners" \
    --jq '[.runners[] | select(.name|startswith("fxrun-drdave-TRX50-AI-TOP-flexnetos")) | {id,name,status,busy,labels:[.labels[].name]}] | sort_by(.name)' \
    | tee "$OUT/api-$name.json" >/dev/null
}

service_show() {
  local name="$1"
  local unit
  shift
  : > "$OUT/systemd-$name.txt"
  for unit in "$@"; do
    {
      echo "--- $unit"
      systemctl show "$unit" -p ActiveState -p SubState -p MainPID -p WorkingDirectory -p ExecStart -p DropInPaths --no-pager || true
    } >> "$OUT/systemd-$name.txt"
  done
}

set_peer_state() {
  local target_unit="$1"
  local action="$2"
  if [[ "$ISOLATE" != 1 || -z "$target_unit" ]]; then return 0; fi
  echo "[$(iso_now)] $action $target_unit"
  sudo systemctl "$action" "$target_unit"
  sleep 4
}

wait_runner_api_state() {
  local runner="$1" want_busy="$2" deadline=$(( $(date -u +%s) + 60 ))
  while true; do
    local state busy status
    state="$(gh api "/orgs/${REPO%%/*}/actions/runners" --jq ".runners[] | select(.name == \"$runner\") | {status,busy}")"
    status="$(jq -r '.status // "missing"' <<<"$state")"
    busy="$(jq -r '.busy // true' <<<"$state")"
    [[ "$status" == "online" && "$busy" == "$want_busy" ]] && return 0
    (( $(date -u +%s) > deadline )) && return 0
    echo "[$(iso_now)] waiting for $runner status=online busy=$want_busy (now status=$status busy=$busy)"
    sleep 3
  done
}

run_list_ids() {
  gh run list --repo "$REPO" --workflow "$WORKFLOW" --branch "$REF" --event workflow_dispatch --limit 20 --json databaseId \
    --jq '[.[].databaseId]'
}

dispatch_probe() {
  local runner="$1" slot="$2" before_ids="$3" dispatch_iso="$4" out run_id
  out="$(gh workflow run "$WORKFLOW" --repo "$REPO" --ref "$REF" -f expected_runner="$runner" -f expected_slot="$slot" 2>&1 || true)"
  printf '%s\n' "$out" > "$OUT/dispatch-slot-$slot.txt"
  run_id="$(grep -Eo 'actions/runs/[0-9]+' <<<"$out" | tail -1 | awk -F/ '{print $3}')"
  if [[ -n "$run_id" ]]; then
    echo "$run_id"
    return 0
  fi

  local deadline=$(( $(date -u +%s) + 45 ))
  while true; do
    run_id="$(gh run list --repo "$REPO" --workflow "$WORKFLOW" --branch "$REF" --event workflow_dispatch --limit 20 \
      --json databaseId,createdAt \
      --jq --argjson before "$before_ids" --arg since "$dispatch_iso" '[.[] | select((.databaseId as $id | $before | index($id) | not) and (.createdAt >= $since))] | sort_by(.createdAt) | reverse | .[0].databaseId // empty')"
    if [[ -n "$run_id" ]]; then
      echo "$run_id"
      return 0
    fi
    if (( $(date -u +%s) > deadline )); then
      echo "failed to discover dispatched run for slot $slot; gh output stored in $OUT/dispatch-slot-$slot.txt" >&2
      return 1
    fi
    sleep 3
  done
}

wait_run() {
  local run_id="$1" start_ms="$2" deadline=$(( $(date -u +%s) + TIMEOUT_SECS ))
  while true; do
    local view status conclusion elapsed_ms
    view="$(gh run view "$run_id" --repo "$REPO" --json status,conclusion,updatedAt,url)"
    status="$(jq -r '.status' <<<"$view")"
    conclusion="$(jq -r '.conclusion // ""' <<<"$view")"
    elapsed_ms="$(ms_delta "$start_ms" "$(now_ms)")"
    echo "[$(iso_now)] run=$run_id status=$status conclusion=${conclusion:-pending} elapsed=$(ms_fmt "$elapsed_ms")"
    [[ "$status" == "completed" ]] && break
    if (( $(date -u +%s) > deadline )); then
      echo "timeout waiting for run $run_id" >&2
      return 1
    fi
    sleep "$POLL_SECS"
  done
}

pull_diag() {
  local slot="$1" root="$2" out="$OUT/diag-slot-$slot.log"
  : > "$out"
  if [[ -d "$root/_diag" ]]; then
    find "$root/_diag" -maxdepth 1 -type f -name '*.log' -printf '%T@ %p\n' \
      | sort -nr | head -5 | cut -d' ' -f2- \
      | while read -r f; do
          echo "--- $f" >> "$out"
          tail -100 "$f" >> "$out" || true
        done
  fi
}

extract_task_output() {
  local log="$1"
  sed -r 's/\x1B\[[0-9;]*[mK]//g' "$log" \
    | awk -F'\t' '$3 ~ /^[0-9TZ:.,:-]+ (expected_runner|actual_runner|expected_slot|runner_os|runner_arch|runner_tracking_id|runner_workspace|hostname|whoami|date_utc)=/ {sub(/^[^ ]+ /, "", $3); print $3}' \
    | jq -Rn 'reduce inputs as $line ({}; ($line | capture("(?<k>[^=]+)=(?<v>.*)")) as $m | .[$m.k] = $m.v)'
}

ALL_UNITS=()
for spec in "${RUNNERS[@]}"; do
  IFS=: read -r _slot _runner _unit _work _install <<<"$spec"
  ALL_UNITS+=("$_unit")
done

cleanup() {
  if [[ "$ISOLATE" == 1 ]]; then
    local u
    for u in "${ALL_UNITS[@]}"; do
      sudo systemctl start "$u" >/dev/null 2>&1 || true
    done
  fi
}
trap cleanup EXIT

api_snapshot before
service_show before "${ALL_UNITS[@]}"

for spec in "${RUNNERS[@]}"; do
  IFS=: read -r slot runner unit work install <<<"$spec"
  peer_unit=""
  for peer in "${RUNNERS[@]}"; do
    IFS=: read -r pslot _prunner punit _pwork _pinstall <<<"$peer"
    [[ "$pslot" != "$slot" ]] && peer_unit="$punit"
  done

  echo "=== Evaluating slot $slot / $runner ==="
  set_peer_state "$peer_unit" stop
  sudo systemctl start "$unit"
  sleep 4
  wait_runner_api_state "$runner" false
  api_snapshot "pre-slot-$slot"
  service_show "pre-slot-$slot" "${ALL_UNITS[@]}"

  before_ids="$(run_list_ids)"
  dispatch_ms="$(now_ms)"
  dispatch_iso="$(iso_now)"
  run_id="$(dispatch_probe "$runner" "$slot" "$before_ids" "$dispatch_iso")"
  visible_ms="$(now_ms)"
  run_url="https://github.com/$REPO/actions/runs/$run_id"
  echo "[$(iso_now)] dispatched run $run_id for $runner: $run_url"

  wait_run "$run_id" "$dispatch_ms"

  run_json="$OUT/run-$slot-$run_id.json"
  run_log="$OUT/run-$slot-$run_id.log"
  gh run view "$run_id" --repo "$REPO" --json databaseId,name,displayTitle,event,headBranch,headSha,status,conclusion,createdAt,updatedAt,url,jobs > "$run_json"
  gh run view "$run_id" --repo "$REPO" --log > "$run_log"

  conclusion="$(jq -r '.conclusion // "unknown"' "$run_json")"
  created_at="$(jq -r '.createdAt' "$run_json")"
  updated_at="$(jq -r '.updatedAt' "$run_json")"
  job_started="$(jq -r '.jobs[0].startedAt' "$run_json")"
  job_completed="$(jq -r '.jobs[0].completedAt' "$run_json")"
  job_id="$(jq -r '.jobs[0].databaseId' "$run_json")"
  run_created_ms="$(epoch_ms "$created_at")"
  job_started_ms="$(epoch_ms "$job_started")"
  job_completed_ms="$(epoch_ms "$job_completed")"
  dispatch_to_visible_ms="$(ms_delta "$dispatch_ms" "$visible_ms")"
  dispatch_to_created_ms="$(ms_delta "$dispatch_ms" "$run_created_ms")"
  queue_wait_ms="$(ms_delta "$run_created_ms" "$job_started_ms")"
  exec_ms="$(ms_delta "$job_started_ms" "$job_completed_ms")"
  total_ms="$(ms_delta "$dispatch_ms" "$job_completed_ms")"

  task_output="$(extract_task_output "$run_log")"
  expected_runner_seen="$(jq --arg runner "$runner" 'if .expected_runner == $runner then 1 else 0 end' <<<"$task_output")"
  actual_runner_seen="$(jq --arg runner "$runner" 'if .actual_runner == $runner then 1 else 0 end' <<<"$task_output")"
  workspace_seen="$(jq --arg work "$work/" 'if (.runner_workspace // "") | startswith($work) then 1 else 0 end' <<<"$task_output")"
  accuracy="fail"
  if [[ "$conclusion" == "success" && "$expected_runner_seen" == 1 && "$actual_runner_seen" == 1 && "$workspace_seen" == 1 ]]; then
    accuracy="pass"
  fi

  steps_json="$(jq '[.jobs[0].steps[]? | {name,status,conclusion,startedAt,completedAt,durationMs: (if (.startedAt and .completedAt) then (((.completedAt|fromdateiso8601) - (.startedAt|fromdateiso8601)) * 1000 | floor) else 0 end)}]' "$run_json")"
  failures_json="$(jq '[.jobs[0].steps[]? | select(.conclusion != "success") | {name,status,conclusion}]' "$run_json")"
  lessons_json="$(jq -n --arg accuracy "$accuracy" --arg conclusion "$conclusion" --arg runner "$runner" --argjson queue "$queue_wait_ms" --argjson total "$total_ms" '
    [
      (if $accuracy == "pass" then "identity and repo-local workspace assertions passed" else "accuracy failed; inspect run log assertions" end),
      (if $conclusion == "success" then "workflow completed successfully" else "workflow did not complete successfully" end),
      (if $queue < 10000 then "runner pickup latency is below 10s" else "runner pickup latency is elevated; inspect capacity, queued work, or GitHub queueing" end),
      (if $total < 60000 then "end-to-end turnaround is below 60s" else "end-to-end turnaround is elevated" end)
    ]')"

  journal_since="$(date -u -d "@$(( dispatch_ms / 1000 - 10 ))" '+%Y-%m-%d %H:%M:%S')"
  journalctl -u "$unit" --since "$journal_since" --no-pager > "$OUT/journal-slot-$slot.log" || true
  pull_diag "$slot" "$install"

  jq -cn \
    --arg slot "$slot" \
    --arg runner "$runner" \
    --arg unit "$unit" \
    --arg run_id "$run_id" \
    --arg job_id "$job_id" \
    --arg run_url "$run_url" \
    --arg dispatch_iso "$dispatch_iso" \
    --arg created_at "$created_at" \
    --arg updated_at "$updated_at" \
    --arg job_started "$job_started" \
    --arg job_completed "$job_completed" \
    --arg conclusion "$conclusion" \
    --arg accuracy "$accuracy" \
    --arg work_dir "$work" \
    --arg install_dir "$install" \
    --argjson dispatch_to_visible_ms "$dispatch_to_visible_ms" \
    --argjson dispatch_to_created_ms "$dispatch_to_created_ms" \
    --argjson pickup_latency_ms "$queue_wait_ms" \
    --argjson exec_ms "$exec_ms" \
    --argjson total_ms "$total_ms" \
    --argjson expected_runner_seen "$expected_runner_seen" \
    --argjson actual_runner_seen "$actual_runner_seen" \
    --argjson workspace_seen "$workspace_seen" \
    --argjson task_output "$task_output" \
    --argjson steps "$steps_json" \
    --argjson failures "$failures_json" \
    --argjson lessons "$lessons_json" \
    '{slot:$slot,runner:$runner,unit:$unit,run_id:$run_id,job_id:$job_id,run_url:$run_url,dispatch_iso:$dispatch_iso,created_at:$created_at,updated_at:$updated_at,job_started:$job_started,job_completed:$job_completed,conclusion:$conclusion,accuracy:$accuracy,work_dir:$work_dir,install_dir:$install_dir,timings_ms:{dispatch_to_visible_ms:$dispatch_to_visible_ms,dispatch_to_created_ms:$dispatch_to_created_ms,pickup_latency_ms:$pickup_latency_ms,exec_ms:$exec_ms,total_ms:$total_ms},assertions:{expected_runner_seen:$expected_runner_seen,actual_runner_seen:$actual_runner_seen,workspace_seen:$workspace_seen},task_output:$task_output,steps:$steps,failures:$failures,lessons:$lessons}' \
    | tee -a "$METRICS" >/dev/null

  printf '| %s | `%s` | %s | %s | %s | %s | %s | %s | %s | [%s](%s) |\n' \
    "$slot" "$runner" "$conclusion" "$accuracy" \
    "$(ms_fmt "$dispatch_to_visible_ms")" "$(ms_fmt "$dispatch_to_created_ms")" "$(ms_fmt "$queue_wait_ms")" "$(ms_fmt "$exec_ms")" "$(ms_fmt "$total_ms")" \
    "$run_id" "$run_url" >> "$SUMMARY"

  set_peer_state "$peer_unit" start
done

api_snapshot after
service_show after "${ALL_UNITS[@]}"

{
  echo
  echo "## Task Output Observed"
  echo
  jq -r '"### " + .runner, "", "```json", (.task_output | tojson), "```", ""' "$METRICS"
  echo "## Failures"
  echo
  if jq -e 'select((.failures | length) > 0)' "$METRICS" >/dev/null; then
    jq -r 'select((.failures | length) > 0) | "### " + .runner, (.failures | tojson), ""' "$METRICS"
  else
    echo "No step failures recorded."
  fi
  echo
  echo "## Final GitHub Runner API Snapshot"
  echo
  echo '```json'
  cat "$OUT/api-after.json"
  echo '```'
  echo
  echo "## Metrics JSONL"
  echo
  echo '```json'
  cat "$METRICS"
  echo '```'
  echo
  echo "## Lessons Learned"
  echo
  jq -r '.runner as $r | .lessons[] | "- [" + $r + "] " + .' "$METRICS"
  echo
  echo "## Artifact Directory"
  echo
  echo "\`$OUT\`"
} >> "$SUMMARY"

echo "Evaluation complete: $OUT"
echo "Summary: $SUMMARY"
