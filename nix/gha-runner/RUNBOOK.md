# FlexNetOS Nix runner operations

This runbook covers the canonical, foreground-only runner. It does not promise boot persistence.

## 1. Owner fences

- `envctl`, `secretctl`, and `secretd` must be present in the one active Nix foundation profile.
- The USB-gated vault must be unlocked; minting fails closed while it is locked.
- The FlexNetOS GitHub App installation must grant `organization_self_hosted_runners:write`.
- Starting a real listener and dispatching a workflow are GitHub organization mutations; perform
  them only for the intended branch/run.

## 2. Preflight

```nu
cd /home/flexnetos/meta/flexnetos_runner/nix/gha-runner
bun run verify.mjs
nix run .#runner -- doctor
nu scripts/mint-runner-token.nu --dry-run
```

All commands must exit zero. The dry run describes the exchange and mints nothing.

## 3. Start the foreground listener

```nu
nix run .#start
```

The start closure:

1. Reuses valid `.runner` plus `.credentials` state from the current profile runtime.
2. Otherwise asks envctl for an App installation token and exchanges it for a runner
   registration token.
3. Registers `flexnetos-nix` at the FlexNetOS organization with custom labels
   `flexnetos,nix` and `--replace --disableupdate`.
4. Runs `Runner.Listener` in manual foreground mode.

Token values must never be echoed, copied into a file, or included in evidence.

## 4. Verify

In another terminal:

```nu
gh api orgs/FlexNetOS/actions/runners --jq '.runners[] | select(.name == "flexnetos-nix") | {name,status,busy,labels:[.labels[].name]}'
```

Expected: `status` is `online`, and labels include `self-hosted`, `flexnetos`, and `nix`.

To execute the repository smoke workflow after the branch is pushed:

```nu
gh workflow run runner-smoke.yml --repo FlexNetOS/flexnetos_runner --ref agent/canonical-nix-runner -f expected_runner=flexnetos-nix
let run_id = (gh run list --repo FlexNetOS/flexnetos_runner --workflow runner-smoke.yml --branch agent/canonical-nix-runner --limit 1 --json databaseId --jq '.[0].databaseId' | str trim)
gh run watch $run_id --repo FlexNetOS/flexnetos_runner --exit-status
```

Record only the run URL/ID, conclusion, commit SHA, and gate counts.

## 5. Stop and recover

`Ctrl-C` stops the listener. The Nix store performs no background activation.

After a crash in the same boot, rerun `nix run .#start`; existing valid registration state is
reused. After reboot, profile-runtime state is gone: unlock the vault and rerun the same command.
The closure re-mints and re-registers idempotently. If the broker remains locked, the command exits
non-zero and must stay stopped.

## 6. Deliberate non-feature: reboot autostart

Unattended reboot startup is not implementable under `NO_SYSTEM_DEPTHS`: immutable store content
cannot execute itself, and every guaranteed boot hook requires an external supervisor. Do not add
a host unit, user unit, linger, cron, desktop autostart, or container restart policy as a workaround.

## 7. Legacy runner retirement fence

The legacy repo-scoped `flexnetos-01` runner is outside this repository and remains untouched until
the owner confirms the canonical runner has serviced the required workflows. Do not deregister,
kill, or archive it as part of source consolidation. Once parity is proven, retire it in a separate
owner-approved operation using a freshly brokered removal token.
