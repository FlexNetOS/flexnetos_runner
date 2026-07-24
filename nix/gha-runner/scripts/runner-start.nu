#!/usr/bin/env nu
# FlexNetOS runner per-session start — idempotent mint → register → run.
#
# Why not just `run`: the runner's mutable state (.runner/.credentials) lives under
# profile-runtime, which is on /run (tmpfs) and is wiped on reboot. A new session must
# re-register from a fresh token when that state is absent. `config.sh --replace` makes
# re-registration idempotent against GitHub's view of the runner.
#
# The flake packages this as `flexnetos-runner-start` (`nix run .#start`). The
# runner launcher and mint script are exact store paths injected by the wrapper,
# so start never depends on a mutable or tmpfs-backed source checkout. It is
# intentionally foreground-only: the Nix store is passive and cannot promise
# unattended reboot activation without an external supervisor.
#
# Owner fence: minting requires the envctl vault unlocked (USB-gated). If the
# vault is locked, mint fails closed and this wrapper exits non-zero. Re-run
# `nix run .#start` after unlock; never fall back to another token path.

def main [] {
    let nu_bin = ($env.GHA_NU? | default "")
    let mint_script = ($env.GHA_MINT_SCRIPT? | default "")
    let runner_launch = ($env.GHA_RUNNER_LAUNCH? | default "")

    if ($nu_bin | is-empty) or ($mint_script | is-empty) or ($runner_launch | is-empty) {
        print -e "[runner-start] closure wiring is incomplete; run the flake's runner-start package."
        exit 2
    }

    let registration = (do { ^$runner_launch is-registered } | complete)
    if $registration.exit_code == 0 {
        print "[runner-start] reusing registered profile-runtime state."
    } else {
        print "[runner-start] minting registration token via envctl (never logged)…"
        let token = (^$nu_bin $mint_script | str trim)
        if ($token | is-empty) {
            print -e "[runner-start] empty token — vault locked or App lacks organization_self_hosted_runners:write. Failing closed."
            exit 2
        }

        print "[runner-start] registering runner (idempotent --replace)…"
        with-env { GHA_RUNNER_TOKEN: $token } {
            ^$runner_launch register
        }
    }

    print "[runner-start] starting foreground listener (executes all workflows)…"
    ^$runner_launch run
}
