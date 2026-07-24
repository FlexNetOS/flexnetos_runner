#!/usr/bin/env nu
# FlexNetOS composed GitHub runner launcher — Nushell, no system-depth installs.
#
# Layers (env injected by the flake app, closure-only — no host PATH):
#   GHA_SUBSTRATE — nix store path of nixpkgs github-runner (the real actions/runner)
#   GHA_BUN       — nix store path of bun (runs the metaharness agent harness)
#
# Path law: all mutable state lives under the profile-runtime link, never a home dot-local dir.
# Secret law: the registration token is minted by envctl and read from the
#   environment as GHA_RUNNER_TOKEN; never hardcoded, logged, or persisted here.
# Runtime law: the harness runs via `bun run`, never bare node/npx.
#
# Commands:
#   doctor            — verify both layers resolve; no token needed
#   is-registered      — exit 0 when reusable runner state exists, else 1
#   register [...]    — configure the runner against the FlexNetOS org (needs GHA_RUNNER_TOKEN)
#   run               — start the registered runner (executes ALL workflows/actions)
#   agent <doctor|init> — drive the metaharness harness layer directly

const ORG_URL = "https://github.com/FlexNetOS"
const LABELS = "flexnetos,nix"

def profile-runtime [] {
    let base = ($env.XDG_RUNTIME_DIR? | default "/run/user/1001")
    $"($base)/yazelix/profile-runtime"
}

# Mutable runner state (config + credentials + work) — profile-runtime, per path law.
def state-dir [] {
    let d = $"(profile-runtime)/gha-runner/state"
    mkdir $d
    $d
}

def work-dir [] {
    let d = $"(profile-runtime)/gha-runner/work"
    mkdir $d
    $d
}

def substrate-bin [name: string] {
    let root = ($env.GHA_SUBSTRATE? | default "")
    if ($root | is-empty) {
        print "[flexnetos-runner] GHA_SUBSTRATE is unset — launch via `nix run .#runner`, not bare nu."
        exit 2
    }
    let p = ($root | path join "bin" $name)
    if not ($p | path exists) {
        print $"[flexnetos-runner] substrate binary missing: ($p)"
        print $"  available: (ls ($root | path join 'bin') | get name | str join ', ')"
        exit 2
    }
    $p
}

def require-token [] {
    if ($env.GHA_RUNNER_TOKEN? | default "" | is-empty) {
        print "[flexnetos-runner] GHA_RUNNER_TOKEN is unset."
        print "  Mint it with `nu scripts/mint-runner-token.nu`; envctl is the sole mint owner."
        print "  This launcher reads it from the environment only and never stores it."
        exit 2
    }
}

# Resolve the metaharness harness bin: hermetic nix build first, local scaffold fallback.
def harness-cli [] {
    let hermetic = ($env.GHA_HARNESS? | default "")
    if not ($hermetic | is-empty) { return $hermetic }
    let local = ($env.PWD | path join "harness" "bin" "cli.js")
    if ($local | path exists) { $local } else { "flexnetos-runner" }
}

def bun-bin [] {
    $env.GHA_BUN? | default "bun"
}

def cmd-doctor [] {
    let sd = (state-dir)
    let wd = (work-dir)
    print $"[flexnetos-runner] state-dir: ($sd)"
    print $"[flexnetos-runner] work-dir:  ($wd)"
    let listener = (substrate-bin "Runner.Listener")
    print $"[flexnetos-runner] substrate: ($env.GHA_SUBSTRATE)"
    ^$listener --version | lines | first | print $"[flexnetos-runner] actions/runner version: ($in)"
    let configured = ($sd | path join ".runner" | path exists)
    print $"[flexnetos-runner] registered: ($configured)"
    let token_set = (($env.GHA_RUNNER_TOKEN? | default "" | is-empty) == false)
    print $"[flexnetos-runner] GHA_RUNNER_TOKEN present: ($token_set)"
    print $"[flexnetos-runner] labels on register: self-hosted + ($LABELS) → ($ORG_URL)"
    print "[flexnetos-runner] doctor: substrate OK"
}

def cmd-register [extra: list<string>] {
    require-token
    let sd = (state-dir)
    let cfg = (substrate-bin "config.sh")
    cd $sd
    # --disableupdate: the runner lives in the immutable nix store and cannot
    # self-update; version bumps come through nixpkgs, not GitHub's auto-update
    # (which would try to rewrite the read-only store path and crash the listener).
    ^$cfg --unattended --replace ...[
        --url $ORG_URL
        --token $env.GHA_RUNNER_TOKEN
        --name "flexnetos-nix"
        --labels $LABELS
        --work (work-dir)
        --disableupdate
    ] ...$extra
}

def cmd-is-registered [] {
    let sd = (state-dir)
    let registered = (($sd | path join ".runner" | path exists)
        and ($sd | path join ".credentials" | path exists))
    if not $registered { exit 1 }
}

def cmd-run [] {
    let sd = (state-dir)
    if not (($sd | path join ".runner") | path exists) {
        print "[flexnetos-runner] not registered yet — run `nix run .#runner -- register` first."
        exit 2
    }
    let listener = (substrate-bin "Runner.Listener")
    cd $sd
    with-env { RUNNER_ROOT: $sd } {
        ^$listener run --startuptype manual
    }
}

def cmd-agent [sub: string] {
    let cli = (harness-cli)
    let bun = (bun-bin)
    match $sub {
        "doctor" => { ^$bun run $cli doctor }
        "init"   => { with-env { RUNNER_WORKDIR: (work-dir) } { ^$bun run $cli init } }
        _ => {
            print $"[flexnetos-runner] unknown agent command: ($sub) — expected doctor|init"
            exit 2
        }
    }
}

def main [
    command: string = "doctor"   # doctor | is-registered | register | run | agent
    ...args: string
] {
    # Pin RUNNER_ROOT for every substrate call (incl. doctor's --version), so the
    # nixpkgs wrapper never falls back to its ~/.github-runner default (path law).
    $env.RUNNER_ROOT = (state-dir)
    match $command {
        "doctor"   => { cmd-doctor }
        "is-registered" => { cmd-is-registered }
        "register" => { cmd-register $args }
        "run"      => { cmd-run }
        "agent"    => { cmd-agent ($args | get 0? | default "doctor") }
        _ => {
            print $"[flexnetos-runner] unknown command: ($command) — expected doctor|is-registered|register|run|agent"
            exit 2
        }
    }
}
