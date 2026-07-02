#!/usr/bin/env bash
# Cache maintenance wrapper for local runners and GitHub Actions.
# Defaults to dry-run audit; set FXRUN_CACHE_MAINTENANCE_MODE=compress and
# FXRUN_CACHE_DRY_RUN=0 only during an approved idle window.
set -euo pipefail

ROOT="${FXRUN_CACHE_ROOT:-_work}"
SLOT="${FXRUN_CACHE_SLOT:-all}"
MIN_AGE="${FXRUN_CACHE_MIN_AGE:-7d}"
FORMAT="${FXRUN_CACHE_FORMAT:-json}"
MODE="${FXRUN_CACHE_MAINTENANCE_MODE:-audit}"
DRY_RUN="${FXRUN_CACHE_DRY_RUN:-1}"
FXRUN_BIN="${FXRUN_BIN:-$(command -v fxrun || true)}"
if [[ -z "${FXRUN_BIN}" ]]; then
  if [[ -x target/debug/fxrun ]]; then
    FXRUN_BIN=target/debug/fxrun
  else
    echo "fxrun not found; build with cargo build -p runner-cli or set FXRUN_BIN" >&2
    exit 1
  fi
fi

case "${MODE}" in
  audit)
    exec "${FXRUN_BIN}" cache audit --root "${ROOT}" --slot "${SLOT}" --min-age "${MIN_AGE}" --format "${FORMAT}"
    ;;
  compress)
    args=(cache compress --root "${ROOT}" --slot "${SLOT}" --min-age "${MIN_AGE}" --format "${FORMAT}")
    if [[ "${DRY_RUN}" != "0" ]]; then
      args+=(--dry-run)
    fi
    exec "${FXRUN_BIN}" "${args[@]}"
    ;;
  restore)
    : "${FXRUN_CACHE_RESTORE_MANIFEST:?set FXRUN_CACHE_RESTORE_MANIFEST for restore mode}"
    args=(cache restore --root "${ROOT}" --manifest "${FXRUN_CACHE_RESTORE_MANIFEST}" --format "${FORMAT}")
    if [[ "${FXRUN_CACHE_RESTORE_ALL:-1}" == "1" ]]; then
      args+=(--all)
    fi
    if [[ "${DRY_RUN}" != "0" ]]; then
      args+=(--dry-run)
    fi
    exec "${FXRUN_BIN}" "${args[@]}"
    ;;
  *)
    echo "unknown FXRUN_CACHE_MAINTENANCE_MODE=${MODE}; expected audit|compress|restore" >&2
    exit 1
    ;;
esac
