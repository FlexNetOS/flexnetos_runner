#!/usr/bin/env bash
# Portable FlexNetOS runner service installer.
#
# The install prefix is the source of truth. This script generates runner .path
# files and systemd units that point back into that prefix; /etc is only an
# optional host-supervisor adapter in --mode system.
# Required prefix-owned state:
#   _work/repos/actions-runner-01
#   _work/repos/actions-runner-02
#   _work/actions-runner-01-work
#   _work/actions-runner-02-work
#   _work/runner-home-01
#   _work/runner-home-02
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
default_prefix="$(cd "${script_dir}/.." && pwd)"
prefix="${FXRUN_RUNNER_PREFIX:-$default_prefix}"
mode="${FXRUN_RUNNER_MODE:-user}"
apply=0
dry_run=0
enable=1
enable_linger=0
runner_user="${FXRUN_RUNNER_USER:-flexnetos}"
unit_prefix="${FXRUN_RUNNER_UNIT_PREFIX:-flexnetos-runner}"
slots=(01 02)
yazelix_bin="${FXRUN_RUNNER_YAZELIX_BIN:-}"
nix_profile_bin="${FXRUN_RUNNER_NIX_PROFILE_BIN:-/nix/var/nix/profiles/default/bin}"
system_path_tail="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/usr/games:/usr/local/games:/snap/bin"
codex_home="${CODEX_HOME:-}"
gh_config_dir="${GH_CONFIG_DIR:-}"
codex_bin_dir="${FXRUN_RUNNER_CODEX_BIN_DIR:-}"
unit_config_home="${FXRUN_RUNNER_XDG_CONFIG_HOME:-}"
kache_rustc_wrapper="${FXRUN_KACHE_RUSTC_WRAPPER:-}"

usage() {
  cat <<USAGE
Usage: $0 --prefix PATH --mode user|system [options]

Generates FlexNetOS self-hosted runner service units from an install prefix.
The runner binaries, workspaces, homes, generated .path files, Codex/GH auth
wiring, and persistent _work state remain under the prefix.

Options:
  --prefix PATH          Release/install prefix. Default: script parent.
  --mode user|system     user systemd preferred; system systemd fallback.
  --apply                Write units/.path files and run systemctl.
  --dry-run              Print generated units, paths, and commands only.
  --no-enable            Write files but do not enable/start services.
  --enable-linger        In user mode, print/run loginctl enable-linger handoff.
  --runner-user USER     Non-root runner user for system units. Default: flexnetos.
  --unit-prefix NAME     systemd template prefix. Default: flexnetos-runner.
  --slot SLOT            Add a runner slot (repeatable). Default: 01, 02.
  --yazelix-bin DIR      Release/Yazelix/Nix bin dir for generated .path.
  --codex-home DIR       CODEX_HOME to place in units.
  --gh-config-dir DIR    GH_CONFIG_DIR to place in units.
  --codex-bin-dir DIR    Codex binary dir to include in generated .path.
  --kache-wrapper FILE   Profile-owned kache-rustc-wrapper executable.
  --xdg-config-home DIR  Config home for user-mode unit placement.
  -h, --help             Show this help.

Examples:
  $0 --prefix /srv/flexnetos_runner --mode user --dry-run
  $0 --prefix /srv/flexnetos_runner --mode user --apply
  sudo $0 --prefix /srv/flexnetos_runner --mode system --apply

Optional root boundary:
  User mode does not require sudo to install or start units. If runners must
  survive logout/boot without a session, run the explicit handoff:
    sudo loginctl enable-linger <user>
USAGE
}

slots_overridden=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix) prefix="$2"; shift 2 ;;
    --mode) mode="$2"; shift 2 ;;
    --apply) apply=1; dry_run=0; shift ;;
    --dry-run) dry_run=1; shift ;;
    --no-enable) enable=0; shift ;;
    --enable-linger) enable_linger=1; shift ;;
    --runner-user) runner_user="$2"; shift 2 ;;
    --unit-prefix) unit_prefix="$2"; shift 2 ;;
    --slot)
      if [[ "$slots_overridden" == 0 ]]; then
        slots=()
        slots_overridden=1
      fi
      slots+=("$2")
      shift 2
      ;;
    --yazelix-bin) yazelix_bin="$2"; shift 2 ;;
    --codex-home) codex_home="$2"; shift 2 ;;
    --gh-config-dir) gh_config_dir="$2"; shift 2 ;;
    --codex-bin-dir) codex_bin_dir="$2"; shift 2 ;;
    --kache-wrapper) kache_rustc_wrapper="$2"; shift 2 ;;
    --xdg-config-home) unit_config_home="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

case "$mode" in
  user|system) ;;
  *) echo "--mode must be user or system" >&2; exit 2 ;;
esac

if [[ "$apply" == 0 && "$dry_run" == 0 ]]; then
  dry_run=1
fi

prefix="${prefix%/}"
if [[ -z "$prefix" ]]; then
  echo "--prefix must not be empty" >&2
  exit 2
fi
if [[ "${#slots[@]}" -eq 0 ]]; then
  echo "at least one --slot is required" >&2
  exit 2
fi

resolve_user_home() {
  local user="$1"
  if [[ "$mode" == "user" ]]; then
    printf '%s\n' "${HOME:?HOME is required for user mode}"
    return 0
  fi
  local resolved_home=""
  if command -v getent >/dev/null 2>&1; then
    resolved_home="$(getent passwd "$user" | awk -F: '{print $6}')"
  fi
  if [[ -n "$resolved_home" ]]; then
    printf '%s\n' "$resolved_home"
    return 0
  fi
  if [[ -n "${HOME:-}" ]]; then
    printf '%s\n' "$HOME"
    return 0
  fi
  return 1
}

runner_home_base="$(resolve_user_home "$runner_user")"
if [[ -z "$runner_home_base" ]]; then
  echo "could not resolve home for runner user $runner_user" >&2
  exit 1
fi

if [[ -z "$codex_home" ]]; then
  codex_home="${runner_home_base}/.codex"
fi
if [[ -z "$gh_config_dir" ]]; then
  gh_config_dir="${runner_home_base}/.config/gh"
fi
if [[ -z "$codex_bin_dir" ]]; then
  codex_bin_dir="${runner_home_base}/.local/bin"
fi
if [[ -z "$yazelix_bin" ]]; then
  yazelix_bin="${prefix}/usr/bin"
fi
if [[ -z "$unit_config_home" ]]; then
  unit_config_home="${runner_home_base}/.config"
fi
if [[ -z "$kache_rustc_wrapper" ]]; then
  kache_rustc_wrapper="${runner_home_base}/.nix-profile/bin/kache-rustc-wrapper"
fi

unit_names=()
for slot in "${slots[@]}"; do
  unit_names+=("${unit_prefix}@${slot}")
done

join_units() {
  local IFS=' '
  echo "${unit_names[*]}"
}

path_for_slot() {
  local slot="$1"
  printf '%s:%s:%s:%s:%s:%s\n' \
    "$yazelix_bin" \
    "${prefix}/_work/runner-home-${slot}/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin" \
    "${prefix}/_work/runner-home-${slot}/.cargo/bin" \
    "$codex_bin_dir" \
    "$nix_profile_bin" \
    "$system_path_tail"
}

unit_body() {
  local include_user="$1"
  cat <<UNIT
[Unit]
Description=FlexNetOS portable GitHub Actions runner slot %i
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=${prefix}/_work/repos/actions-runner-%i/flexnetos-runner-entrypoint.sh
WorkingDirectory=${prefix}/_work/repos/actions-runner-%i
PassEnvironment=DBUS_SESSION_BUS_ADDRESS
PassEnvironment=DISPLAY
PassEnvironment=WAYLAND_DISPLAY
PassEnvironment=XDG_RUNTIME_DIR
PassEnvironment=XDG_SESSION_TYPE
Environment=HOME=${prefix}/_work/runner-home-%i
Environment=GIT_CONFIG_GLOBAL=${prefix}/_work/runner-home-%i/.gitconfig
Environment=XDG_CONFIG_HOME=${prefix}/_work/runner-home-%i/.config
Environment=XDG_CACHE_HOME=${prefix}/_work/runner-home-%i/.cache
Environment=CARGO_HOME=${prefix}/_work/runner-home-%i/.cargo
Environment=CARGO_BUILD_RUSTC_WRAPPER=${prefix}/_work/kache-shims/flexnetos-kache-rustc-wrapper-%i
Environment=RUSTUP_HOME=${prefix}/_work/runner-home-%i/.rustup
Environment=BUN_INSTALL=${prefix}/_work/runner-home-%i/.bun
Environment=BUN_TMPDIR=${prefix}/_work/runner-home-%i/.cache/bun/tmp
Environment=KACHE_CONFIG=${prefix}/_work/runner-home-%i/.config/kache/config.toml
Environment=KACHE_CACHE_DIR=${prefix}/_work/runner-home-%i/.cache/kache
Environment=CODEX_HOME=${codex_home}
Environment=GH_CONFIG_DIR=${gh_config_dir}
Environment=RUNNER_WORKSPACE=${prefix}/_work/actions-runner-%i-work
KillMode=process
KillSignal=SIGTERM
TimeoutStopSec=5min
UNIT
  if [[ "$include_user" == 1 ]]; then
    printf 'User=%s\n' "$runner_user"
  fi
  cat <<'UNIT'

[Install]
WantedBy=default.target
UNIT
}

unit_dir_for_mode() {
  case "$mode" in
    user) printf '%s\n' "${unit_config_home}/systemd/user" ;;
    system) printf '%s\n' "/etc/systemd/system" ;;
  esac
}

unit_path="$(unit_dir_for_mode)/${unit_prefix}@.service"
include_user=0
if [[ "$mode" == "system" ]]; then
  include_user=1
fi

print_plan() {
  echo "# FlexNetOS portable runner install plan"
  echo "prefix=${prefix}"
  echo "mode=${mode}"
  echo "slots=${slots[*]}"
  echo "unit=${unit_path}"
  echo "codex_home=${codex_home}"
  echo "gh_config_dir=${gh_config_dir}"
  echo "kache_rustc_wrapper=${kache_rustc_wrapper}"
  echo
  echo "# generated runner .path files"
  for slot in "${slots[@]}"; do
    echo "## ${prefix}/_work/repos/actions-runner-${slot}/.path"
    path_for_slot "$slot"
  done
  echo
  echo "# generated runner .env files"
  for slot in "${slots[@]}"; do
    echo "## ${prefix}/_work/repos/actions-runner-${slot}/.env"
    printf 'LANG=en_US.UTF-8\n'
    printf 'ACTIONS_RUNNER_HOOK_JOB_STARTED=%s/scripts/runner-repo-guard.sh\n' "$prefix"
    printf 'FXRUN_REPO_BLOCKLIST=%s/_work/config/runner-blocklist.txt\n' "$prefix"
  done
  echo
  echo "# generated systemd unit"
  echo "## ${unit_path}"
  unit_body "$include_user"
  echo
  echo "# activation commands"
  case "$mode" in
    user)
      echo "systemctl --user daemon-reload"
      if [[ "$enable" == 1 ]]; then
        echo "systemctl --user enable --now $(join_units)"
      fi
      if [[ "$enable_linger" == 1 ]]; then
        echo "sudo loginctl enable-linger ${runner_user}"
      else
        echo "# optional root handoff for boot/login independence: sudo loginctl enable-linger ${runner_user}"
      fi
      ;;
    system)
      echo "systemctl daemon-reload"
      if [[ "$enable" == 1 ]]; then
        echo "systemctl enable --now $(join_units)"
      fi
      ;;
  esac
}

write_path_files() {
  local slot runner_dir home_dir work_dir
  if [[ ! -x "$kache_rustc_wrapper" ]]; then
    echo "profile-owned kache wrapper is not executable: $kache_rustc_wrapper" >&2
    exit 1
  fi
  for slot in "${slots[@]}"; do
    runner_dir="${prefix}/_work/repos/actions-runner-${slot}"
    home_dir="${prefix}/_work/runner-home-${slot}"
    work_dir="${prefix}/_work/actions-runner-${slot}-work"
    install -d -m 0755 "$runner_dir" "$home_dir" "$work_dir"
    kache_shim="${prefix}/_work/kache-shims/flexnetos-kache-rustc-wrapper-${slot}"
    install -d -m 0755 "${home_dir}/.cargo/bin" "${home_dir}/.config/kache" "${home_dir}/.cache/kache" "${prefix}/_work/kache-shims"
    # Owner-only break-glass path. Automatic runner installation is profile-owned Nushell.
    # back to its cache manifest between jobs, which deleted the wrapper and broke
    # every cargo job with rustc-wrapper ENOENT (2026-07-09 incident). _work/kache-shims
    # is runner-managed and never touched by job-level cache actions.
    cat > "$kache_shim" <<EOF
#!/usr/bin/env bash
set -euo pipefail
export KACHE_CONFIG="${home_dir}/.config/kache/config.toml"
export KACHE_CACHE_DIR="${home_dir}/.cache/kache"
exec "${kache_rustc_wrapper}" "\$@"
EOF
    chmod 755 "$kache_shim"
    cat > "${home_dir}/.cargo/config.toml" <<EOF
[build]
rustc-wrapper = "$kache_shim"
EOF
    cat > "${home_dir}/.config/kache/config.toml" <<EOF
[cache]
local_store = "${home_dir}/.cache/kache"
local_max_size = "50GiB"
local_only = true
clean_incremental = true
cache_executables = false
EOF
    cat > "${runner_dir}/flexnetos-runner-entrypoint.sh" <<EOF
#!/usr/bin/env bash
set -euo pipefail

current_user="\$(id -un)"

manager_env_value() {
  local key="\$1"
  systemctl --user show-environment 2>/dev/null | awk -F= -v key="\$key" '\$1 == key { print substr(\$0, length(key) + 2); exit }'
}

active_session_id() {
  loginctl list-sessions --no-legend 2>/dev/null | awk -v user="\$current_user" '\$3 == user && \$6 == "user" && \$5 != "manager" { print \$1; exit }'
}

session_property() {
  local key="\$1"
  local session_id
  session_id="\$(active_session_id || true)"
  [[ -n "\$session_id" ]] || return 1
  loginctl show-session "\$session_id" -p "\$key" --value 2>/dev/null
}

if [[ -z "\${DISPLAY:-}" ]]; then
  export DISPLAY="\$(manager_env_value DISPLAY || true)"
fi
if [[ -z "\${WAYLAND_DISPLAY:-}" ]]; then
  export WAYLAND_DISPLAY="\$(manager_env_value WAYLAND_DISPLAY || true)"
fi
if [[ -z "\${XDG_SESSION_TYPE:-}" || "\${XDG_SESSION_TYPE:-}" == "unspecified" ]]; then
  session_type="\$(manager_env_value XDG_SESSION_TYPE || true)"
  if [[ -z "\$session_type" || "\$session_type" == "unspecified" ]]; then
    session_type="\$(session_property Type || true)"
  fi
  if [[ -n "\$session_type" ]]; then
    export XDG_SESSION_TYPE="\$session_type"
  fi
fi

exec "${runner_dir}/runsvc.sh"
EOF
    chmod 755 "${runner_dir}/flexnetos-runner-entrypoint.sh"
    path_for_slot "$slot" > "${runner_dir}/.path"
    cat > "${runner_dir}/.env" <<EOF
LANG=en_US.UTF-8
ACTIONS_RUNNER_HOOK_JOB_STARTED=${prefix}/scripts/runner-repo-guard.sh
FXRUN_REPO_BLOCKLIST=${prefix}/_work/config/runner-blocklist.txt
EOF
  done
}

write_unit() {
  install -d -m 0755 "$(dirname "$unit_path")"
  unit_body "$include_user" > "$unit_path"
}

activate_units() {
  if [[ "$enable" != 1 ]]; then
    return 0
  fi
  case "$mode" in
    user)
      systemctl --user daemon-reload
      systemctl --user enable --now "${unit_names[@]}"
      if [[ "$enable_linger" == 1 ]]; then
        if [[ "${EUID}" -eq 0 ]]; then
          loginctl enable-linger "$runner_user"
        else
          echo "linger requires explicit root handoff: sudo loginctl enable-linger ${runner_user}" >&2
        fi
      fi
      ;;
    system)
      if [[ "${EUID}" -ne 0 ]]; then
        echo "system mode --apply requires root because it writes ${unit_path}" >&2
        exit 1
      fi
      systemctl daemon-reload
      systemctl enable --now "${unit_names[@]}"
      ;;
  esac
}

print_plan

if [[ "$dry_run" == 1 ]]; then
  exit 0
fi

write_path_files
write_unit
activate_units
