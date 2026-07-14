#!/usr/bin/env nu

const banned_cache_terms = [
  "actions/cache"
  "Swatinem/rust-cache"
  "DeterminateSystems/magic-nix-cache"
  "cachix/cachix-action"
  "cache-from: type=gha"
  "cache-to: type=gha"
  "CARGO_INCREMENTAL: 1"
  "RUSTC_WRAPPER: sccache"
  "RUSTC_WRAPPER: ccache"
]

const banned_shell_terms = [
  "shell: bash"
  "shell: sh"
  "shell: zsh"
  ".sh "
  ".sh\n"
  "/bin/bash"
  "/bin/sh"
  "/bin/zsh"
]

def workflow_files [root: path] {
  let workflow_root = ($root | path join ".github" "workflows")
  if not ($workflow_root | path exists) {
    return []
  }
  (glob ($workflow_root | path join "*.yml")) ++ (glob ($workflow_root | path join "*.yaml"))
}

def audit_workflow [file: path] {
  let body = (open --raw $file)
  let break_glass = ($body | str contains "OWNER_BREAK_GLASS_ONLY: disabled until every run step is Nushell.")
  let false_job = ($body | str contains "FLEXNETOS_OWNER_BREAK_GLASS_NON_NUSHELL == 'I_ACCEPT_NON_NUSHELL'")
  let cache_hits = ($banned_cache_terms | where {|term| $body | str contains $term })
  let shell_hits = ($banned_shell_terms | where {|term| $body | str contains $term })
  let cache_rows = ($cache_hits | each {|term| {file: ($file | into string), kind: "cache", term: $term} })
  let shell_rows = if ($shell_hits | is-empty) or ($break_glass and $false_job) {
    []
  } else {
    $shell_hits | each {|term| {file: ($file | into string), kind: "automatic-shell", term: $term} }
  }
  $cache_rows ++ $shell_rows
}

def audit_tracked_work [root: path] {
  ^git -C $root ls-files -- _work
  | lines
  | each {|file| {file: $file, kind: "tracked-runtime-state", term: "_work"} }
}

def main [--root: path = ".", --json] {
  let findings = (
    (workflow_files $root | each {|file| audit_workflow $file } | flatten)
    ++ (audit_tracked_work $root)
  )
  if $json {
    $findings | to json --indent 2 | print
  } else if ($findings | is-empty) {
    print "PASS: workflows are Kache-only; non-Nushell jobs are fail-closed"
  } else {
    $findings | table | print
  }
  if not ($findings | is-empty) {
    error make {msg: $"runner policy violations: ($findings | length)"}
  }
}
