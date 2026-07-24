# Canonical FlexNetOS Nix GitHub runner

This directory is the sole GitHub Actions worker implementation owned by
`FlexNetOS/flexnetos_runner`. Yazelix consumes this flake as a pinned input; it does not carry a
second copy.

## Hard boundary

`NO_SYSTEM_DEPTHS` is absolute. The only sanctioned depth is the Nix store. The runner installs
no host or user unit, writes no `/etc` state, and never enables linger. A Nix store path is passive,
so unattended reboot activation is deliberately unsupported. Start the listener explicitly in a
foreground session; after a reboot, unlock the broker and start it again.

## One closure, two layers

| Layer | Role |
|---|---|
| `nixpkgs#github-runner` | Real GitHub `actions/runner` substrate for all workflows selecting `[self-hosted, flexnetos, nix]`. |
| Metaharness | `@metaharness/kernel`, `@metaharness/host-github-actions`, and `agentic-flow`, invoked by workflows as a step on that substrate. |

Mutable `.runner`, `.credentials`, diagnostics, and work state live below
`$XDG_RUNTIME_DIR/yazelix/profile-runtime/gha-runner`; no credentials are repository or home
state. The pinned runner is launched with self-update disabled because upgrades come through Nix.

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

# Explicit foreground session: mint if needed → register --replace → listen.
nix run .#start

# Agent-layer proof on the same closure.
nix run .#runner -- agent doctor
```

`Ctrl-C` stops the foreground listener. After a crash or reboot, rerun `nix run .#start`; valid
profile-runtime registration state is reused within the same boot and recreated when absent.

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
