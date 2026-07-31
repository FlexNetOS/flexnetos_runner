# Canonical FlexNetOS Nix GitHub runner

This directory is the sole GitHub Actions worker implementation owned by
`FlexNetOS/flexnetos_runner`. Yazelix consumes this flake as a pinned input; it does not carry a
second copy.

## Hard boundary

`NO_SYSTEM_DEPTHS` is absolute. The only sanctioned depth is the Nix store. The runner installs
no host or user unit, writes no `/etc` state, and never enables linger. Yazelix owns activation:
`yzx enter` and `yzx launch` open the USB-backed vault and start the listener from the composed
profile closure.

## One closure, two layers

| Layer | Role |
|---|---|
| `nixpkgs#github-runner` | Real GitHub `actions/runner` substrate for all workflows selecting `[self-hosted, flexnetos, nix]`. |
| Metaharness | `@metaharness/kernel`, `@metaharness/host-github-actions`, and `agentic-flow`, invoked by workflows as a step on that substrate. |

Mutable `.runner`, `.credentials`, diagnostics, and work state live below the persistent
Yazelix runtime `/home/flexnetos/meta/var/lib/yazelix/runtime/runner`; no credentials are
repository, home-dotdir, or host runtime state. The pinned runner is launched with self-update
disabled because upgrades come through Nix.

## Registration authority

`envctl` remains the sole secret/token minter. `scripts/mint-runner-token.nu` asks `secretctl` for
a short-lived GitHub App installation token, exchanges that opaque token with GitHub for an org
runner registration token, and emits only the registration token to its immediate caller. The
private key never enters this process, and no token is committed or logged.

## Commands

Run these from `nix/gha-runner`:

```nu
# Offline source contract.
bun run verify.mjs

# Closure and both layers resolve; no token needed.
nix run .#runner -- doctor

# Describe the broker exchange without minting.
nu scripts/mint-runner-token.nu --dry-run

# Direct diagnostic entrypoint; normal activation is `yzx enter` or `yzx launch`.
nix run .#start

# Agent-layer proof on the same closure.
nix run .#runner -- agent doctor
```

`Ctrl-C` stops a directly invoked listener. Normal recovery is a Yazelix launch; durable
registration state is reused across boots, and Yazelix starts the runner only after the
USB-backed envctl vault and databases are ready.

## Layout

| Path | Role |
|---|---|
| `flake.nix` | Pinned substrate, Metaharness package, `runner`, `start`, and verification apps. |
| `scripts/runner.nu` | `doctor`, `is-registered`, `register`, `run`, and `agent` commands. |
| `scripts/mint-runner-token.nu` | Brokered App-token → runner-registration-token exchange. |
| `scripts/runner-start.nu` | Foreground, idempotent mint → register → listen entrypoint. |
| `harness/` | Hermetic Metaharness package and tests. |
| `verify.mjs` | Offline negative and composition gates. |

## Consumer contract

Yazelix pins this nested flake using
`github:FlexNetOS/flexnetos_runner/<commit>?dir=nix/gha-runner` and composes
`packages.<system>.runner-start` into its single `lifeos_foundation_yzx` profile element. Local
development uses `--override-input` to this checkout; published consumers pin a commit.

See `RUNBOOK.md` for operator proof and recovery.
