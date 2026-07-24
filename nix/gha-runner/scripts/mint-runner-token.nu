#!/usr/bin/env nu
# FlexNetOS runner registration-token mint — non-interactive, envctl-brokered.
#
# Closes the seed→vault→envctl→registration-token chain the runner launcher expects:
# runner.nu's `require-token` reads GHA_RUNNER_TOKEN from the environment; this script
# produces it without any prompt and writes only the final registration token
# to stdout for its immediate caller.
#
# Chain (the App private key stays inside envctl; this script handles only
# short-lived opaque tokens in memory):
#   1. envctl/secretctl mints a short-lived GitHub *App installation* token from the
#      FlexNetOS App (app 4044997, installation 140063898) — the App private key is
#      sealed broker-only in the envctl vault; it never touches this process.
#   2. That installation token calls the GitHub REST API to create a runner
#      *registration* token for the FlexNetOS org.
#   3. The registration token is emitted on stdout (single line) for the caller to
#      capture into GHA_RUNNER_TOKEN, e.g.:
#         GHA_RUNNER_TOKEN=(nu scripts/mint-runner-token.nu) nix run .#runner -- register
#
# Requirements (owner-provisioned — see the runbook's "owner fences"):
#   - secretctl on PATH or in the active nix profile, secretd running, vault UNLOCKED
#     (USB-gated; fail-closed when locked).
#   - The FlexNetOS App must grant `organization_self_hosted_runners: write` so its
#     installation token can create runner registration tokens.
#
# Env overrides (all optional):
#   ENVCTL_INSTALLATION_ID   default 140063898
#   ENVCTL_ORG               default FlexNetOS
#   SECRETCTL_BIN            default `secretctl` on PATH
#   GITHUB_API_BASE          default https://api.github.com

const DEFAULT_INSTALLATION_ID = "140063898"
const DEFAULT_ORG = "FlexNetOS"

def secretctl-bin [] {
    let override = ($env.SECRETCTL_BIN? | default "")
    if not ($override | is-empty) { return $override }
    let which = (which secretctl | get path? | get 0? | default "")
    if ($which | is-empty) {
        print -e "[mint] secretctl not found on PATH. Set SECRETCTL_BIN or install it into the nix profile."
        print -e "[mint] owner fence: secretctl+secretd must be profile-installed and the vault unlocked (USB-gated)."
        exit 2
    }
    $which
}

def main [
    --dry-run   # print the exact commands that would run, mint nothing
] {
    let inst = ($env.ENVCTL_INSTALLATION_ID? | default $DEFAULT_INSTALLATION_ID)
    let org = ($env.ENVCTL_ORG? | default $DEFAULT_ORG)
    let api = ($env.GITHUB_API_BASE? | default "https://api.github.com")

    if $dry_run {
        # Dry-run must not require secretctl to be installed — it only describes the chain.
        let sc = ($env.SECRETCTL_BIN? | default "secretctl")
        print $"# 1. mint App installation token \(broker-only key, never revealed\):"
        print $"($sc) mint-github --installation-id ($inst) --ttl-secs 3600 --output json"
        print $"# 2. exchange for a runner registration token:"
        print $"curl -sS -X POST -H 'Authorization: Bearer <installation-token>' \\"
        print $"  ($api)/orgs/($org)/actions/runners/registration-token"
        print "# 3. emit .token on stdout for GHA_RUNNER_TOKEN"
        return
    }

    let sc = (secretctl-bin)
    # Step 1 — App installation token. stdout is exactly {"token","expires_at_unix"}.
    let mint = (^$sc mint-github --installation-id $inst --ttl-secs 3600 --output json
        | from json)
    let inst_token = ($mint | get token)

    # Step 2 — runner registration token via the GitHub REST API.
    let reg = (http post
        --headers [Authorization $"Bearer ($inst_token)" Accept "application/vnd.github+json" "X-GitHub-Api-Version" "2022-11-28"]
        --content-type application/json
        $"($api)/orgs/($org)/actions/runners/registration-token"
        {})

    # Step 3 — emit only the opaque registration token, nothing else.
    $reg | get token
}
