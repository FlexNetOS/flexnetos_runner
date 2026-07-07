#!/usr/bin/env bash
set -euo pipefail

current_user="$(id -un)"

manager_env_value() {
  local key="$1"
  systemctl --user show-environment 2>/dev/null | awk -F= -v key="$key" '$1 == key { print substr($0, length(key) + 2); exit }'
}

active_session_id() {
  loginctl list-sessions --no-legend 2>/dev/null | awk -v user="$current_user" '$3 == user && $6 == "user" && $5 != "manager" { print $1; exit }'
}

session_property() {
  local key="$1"
  local session_id
  session_id="$(active_session_id || true)"
  [[ -n "$session_id" ]] || return 1
  loginctl show-session "$session_id" -p "$key" --value 2>/dev/null
}

if [[ -z "${DISPLAY:-}" ]]; then
  export DISPLAY="$(manager_env_value DISPLAY || true)"
fi
if [[ -z "${WAYLAND_DISPLAY:-}" ]]; then
  export WAYLAND_DISPLAY="$(manager_env_value WAYLAND_DISPLAY || true)"
fi
if [[ -z "${XDG_SESSION_TYPE:-}" || "${XDG_SESSION_TYPE:-}" == "unspecified" ]]; then
  session_type="$(manager_env_value XDG_SESSION_TYPE || true)"
  if [[ -z "$session_type" || "$session_type" == "unspecified" ]]; then
    session_type="$(session_property Type || true)"
  fi
  if [[ -n "$session_type" ]]; then
    export XDG_SESSION_TYPE="$session_type"
  fi
fi

exec "/home/flexnetos/FlexNetOS/src/flexnetos_runner/_work/repos/actions-runner-02/runsvc.sh"
