#!/usr/bin/env nu

def main [
  --slot: string
  --profile: path = "/home/flexnetos/.nix-profile"
  --runtime-root: path = "/run/user/1001/yazelix/runners"
] {
  if ($slot | str trim | is-empty) {
    error make {msg: "--slot is required"}
  }

  let runner_source = ($profile | path join "libexec" "flexnetos-runner" "actions-runner")
  let fxrun_actions = ($profile | path join "bin" "fxrun-actions")
  let slot_root = ($runtime_root | path join $slot)
  let runner_home = ($slot_root | path join "runner")
  let work_dir = ($slot_root | path join "work")
  let service_home = ($slot_root | path join "home")

  if not ($runner_source | path exists) {
    error make {msg: $"profile runner tree is missing: ($runner_source)"}
  }
  if not ($fxrun_actions | path exists) {
    error make {msg: $"profile runner supervisor is missing: ($fxrun_actions)"}
  }

  if ($slot_root | path exists) {
    rm --recursive --force $slot_root
  }
  mkdir $slot_root $work_dir $service_home
  cp --recursive $runner_source $runner_home

  let exit_code = (try {
    ^$fxrun_actions --home $runner_home --work-dir $work_dir --service-home $service_home --dry-run=false --confirm=true run-once
    $env.LAST_EXIT_CODE
  } catch {|err|
    print --stderr $err.msg
    1
  })

  rm --recursive --force $slot_root
  exit $exit_code
}
