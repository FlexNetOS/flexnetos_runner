#!/usr/bin/env bash
# ACTIONS_RUNNER_HOOK_JOB_STARTED guard: fail-closed repo blocklist for the
# FlexNetOS self-hosted runners. A repo listed in the blocklist file has its
# jobs FAILED at job start on this runner (operator hold), without touching
# org-level runner-group access.
#
# Wiring: set ACTIONS_RUNNER_HOOK_JOB_STARTED to this script before launching
# the Nix-owned listener. The listener remains a foreground, per-session process.
#
# Blocklist: one owner/repo per line; '#' comments and blank lines ignored.
# Default: <runner-prefix>/_work/config/runner-blocklist.txt
# (override: FXRUN_REPO_BLOCKLIST).
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
runner_prefix="$(cd "${script_dir}/.." && pwd)"
BLOCKLIST="${FXRUN_REPO_BLOCKLIST:-${runner_prefix}/_work/config/runner-blocklist.txt}"
repo="${GITHUB_REPOSITORY:-}"

# No repo context or no blocklist => never block (fail-open for the guard's
# own plumbing; the block itself is fail-closed once listed).
[[ -z "$repo" || ! -f "$BLOCKLIST" ]] && exit 0

while IFS= read -r line; do
  line="${line%%#*}"
  line="$(echo "$line" | tr -d '[:space:]')"
  [[ -z "$line" ]] && continue
  if [[ "${repo,,}" == "${line,,}" ]]; then
    echo "::error::repo $repo is on the FlexNetOS runner blocklist ($BLOCKLIST) — operator hold; job refused on this runner."
    exit 1
  fi
done < "$BLOCKLIST"
exit 0
