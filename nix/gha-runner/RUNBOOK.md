# FlexNetOS Nix runner operations

This runbook covers the canonical runner owned and activated by Yazelix.

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

## 3. Start the Yazelix-owned listener

```nu
yzx enter
```

The start closure:

1. Reuses valid `.runner` plus `.credentials` state from `/home/flexnetos/meta/var/lib/yazelix/runtime/runner`.
2. Otherwise asks envctl for an App installation token and exchanges it for a runner
   registration token.
3. Registers `flexnetos-nix` at the FlexNetOS organization with custom labels
   `flexnetos,nix` and `--replace --disableupdate`.
4. Runs `Runner.Listener` under the Yazelix runtime supervisor.

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

Exiting Yazelix stops the interactive session; a subsequent `yzx enter` or `yzx launch` verifies
the owned stack again. Existing valid registration state is reused across reboot. Yazelix unlocks
the vault and starts the same closure automatically.
The closure re-mints and re-registers idempotently. If the broker remains locked, the command exits
non-zero and must stay stopped.

## 6. Activation boundary

The first `yzx enter` or `yzx launch` after login is the sole activation boundary and starts the
complete stack without prompts. Immutable store content does not create a competing boot hook;
do not add a host unit, user unit, linger, cron, desktop autostart, or container restart policy.

## 7. Legacy runner retirement

Legacy host-supervised runner instances are competing owners. Preserve registration/work state in
the Yazelix runtime, stop and remove their local units, and verify the profile-owned listener is
online. Deregistration requires a freshly brokered removal token only when GitHub still lists a
distinct obsolete runner identity.
