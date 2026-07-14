#!/usr/bin/env bash
# Retarget the local FlexNetOS GitHub Actions runner services to the repo-local
# runner directories and flexnetos-owned auth/tooling homes.
set -euo pipefail

repo=/home/flexnetos/meta/src/flexnetos_runner
owner=flexnetos:flexnetos
services=(
  actions.runner.FlexNetOS.fxrun-drdave-TRX50-AI-TOP-flexnetos-01.service
  actions.runner.FlexNetOS.fxrun-drdave-TRX50-AI-TOP-flexnetos-02.service
)

require_root() {
  if [[ "${EUID}" -ne 0 ]]; then
    echo "retarget-local-runner-services.sh must run as root; use sudo" >&2
    exit 1
  fi
}

install_unit() {
  local slot="$1"
  local service="actions.runner.FlexNetOS.fxrun-drdave-TRX50-AI-TOP-flexnetos-${slot}.service"
  local unit="/etc/systemd/system/${service}"
  local dropin_dir="/etc/systemd/system/${service}.d"
  local runner_dir="${repo}/_work/repos/actions-runner-${slot}"
  local home_dir="${repo}/_work/runner-home-${slot}"

  install -d -m 0755 "${dropin_dir}"
  cat > "${unit}" <<UNIT
[Unit]
Description=GitHub Actions Runner (FlexNetOS.fxrun-drdave-TRX50-AI-TOP-flexnetos-${slot})
After=network-online.target

[Service]
ExecStart=${runner_dir}/runsvc.sh
User=flexnetos
WorkingDirectory=${runner_dir}
KillMode=process
KillSignal=SIGTERM
TimeoutStopSec=5min

[Install]
WantedBy=multi-user.target
UNIT
  cat > "${dropin_dir}/10-runner-home.conf" <<DROPIN
[Service]
Environment=HOME=${home_dir}
Environment=GIT_CONFIG_GLOBAL=${home_dir}/.gitconfig
Environment=CODEX_HOME=/home/flexnetos/.codex
Environment=GH_CONFIG_DIR=/home/flexnetos/.config/gh
DROPIN
}

require_root

chown -R "${owner}" \
  "${repo}/_work/repos/actions-runner-01" \
  "${repo}/_work/repos/actions-runner-02" \
  "${repo}/_work/actions-runner-01-work" \
  "${repo}/_work/actions-runner-02-work" \
  "${repo}/_work/runner-home-01" \
  "${repo}/_work/runner-home-02"

install_unit 01
install_unit 02
systemctl daemon-reload
for service in "${services[@]}"; do
  systemctl enable "${service}"
done
for service in "${services[@]}"; do
  systemctl restart "${service}"
done
sleep 10
for service in "${services[@]}"; do
  systemctl is-active --quiet "${service}"
  systemctl show "${service}" -p User -p FragmentPath -p ActiveState -p SubState -p ExecMainStartTimestamp --no-pager
done
