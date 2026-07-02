# flexnetos_runner

The **execution plane** of FlexNetOS's GitHub↔local automation: a local, self-hosted runner that
executes CI/jobs/loops on the developer's own hardware and **connects all of meta** by routing work
to the existing kernels rather than reimplementing them. It is the muscle paired with
[`flexnetos_github_app`](https://github.com/FlexNetOS/flexnetos_github_app)'s control plane.

Design: **ADR-0008** (`handoff/docs/adr-0008-flexnetos-app-runner.md`).

Two shapes, by design:
1. **Self-hosted GitHub Actions runner** — JIT/ephemeral (`generate-jitconfig`, single-job-then-
   removed), with safety rails (non-root, no Docker socket, `_work` on tmpfs, **fork-PR isolation**).
   Productizes the shell scripts extracted from `.github_org/runner/`.
2. **Meta-native dispatcher** — a UDS server receiving **signed** job specs from the App and
   **routing** them to the right kernel: `build/test → loop_lib`, `agent-task/review → atc`,
   `loop-cycle → handoff hf`, `lease/a2a → weave`, `worktrees → meta_git_lib`. It never reimplements
   these — it delegates (ADR-0008 §2).


## Architecture and automation map

```text
GitHub / user intent
        |
        v
+-----------------------+       signed dispatch frame       +----------------------+
| flexnetos_github_app  | -------------------------------> | fxrun-dispatch       |
| control plane         |                                  | local UDS server     |
+-----------------------+                                  +----------+-----------+
                                                                      |
                                                                      v
                          +-------------------------------------------+-------------------+
                          | admission gates: verify -> authority -> lint -> scan -> fork |
                          | -> approval -> quarantine -> route/allowlist -> state/rate   |
                          | -> single-flight -> loop/budget                              |
                          +-------------------------------------------+-------------------+
                                                                      |
                                                                      v
+------------------+       JobSpec stdin/env/cost/status       +--------------------------+
| runner-core      | <----------------------------------------> | SubprocessInvoker        |
| pure policy/data |                                           | fresh workspace + bounds |
+------------------+                                           +------------+-------------+
                                                                      |
                                                                      v
                                                +------------------------------+
                                                | existing kernels             |
                                                | loop / atc / hf / weave      |
                                                +------------------------------+
                                                                      |
                                                                      v
                                                DispatchResponse + redacted NDJSON audit
```

Automation boundaries:
- **Automated now:** signed JobSpec verification, admission gates, routing, kernel spawn, deadline /
  idle enforcement, cost relay, recovery directives, risk/cost audit, Actions runner install/register
  with explicit confirmation.
- **User/operator today:** approvals, budget/quarantine/constitution re-arm, runner install/register
  confirmation, policy/secrets/socket/log configuration.
- **Automated/hardened on this branch:** full-envelope authority signatures, private UDS parent and
  `0600` socket mode, nonce-based fresh workspaces, Actions runner SHA-256 verification, CI cargo
  audit.
- **Planned:** Actions registration token non-argv path, rate-limit clock freshness, structured kernel
  result/status, desktop approval/re-arm flow.

See [`docs/automation-and-user-story.md`](docs/automation-and-user-story.md) for the full component
inventory, data-flow diagrams, fresh backlog, agent automation story, and user communication flow.

## Workspace

| Crate | Bin | Role |
|-------|-----|------|
| `runner-core` | — | Pure core: signed job-spec type, kernel router (delegate-only), fork-PR isolation policy, JIT lifecycle state. Fully unit-tested. |
| `runner-actions` | `fxrun-actions` | Self-hosted Actions runner supervisor (JIT register → run one → deregister). P1. |
| `runner-dispatch` | `fxrun-dispatch` | UDS server: verify signed job spec → route → invoke kernel. P2. |
| `runner-cli` | `fxrun` | Operator CLI: `route`, `agents`, `doctor`. |

## Agent backends (any agent — Claude right now)

Agent-class jobs (`review`, `agent`) route to the `atc` kernel, which drives a selectable
**agent backend**. The runner carries the selected backend as a delegate-only seam; **Weave owns
live model/vendor routing policy** and can inject that choice through `WEAVE_FXRUN_AGENT` or by
minting the signed job field. The runner never runs the model itself, so `atc` owns the spawn.
Supported, with the current (June 2026) headless invocation `atc` uses:

| Agent | API | Spawn (`atc`) |
|-------|-----|---------------|
| `claude` *(default)* | Anthropic native | `claude -p --bare --permission-mode dontAsk --output-format json --model claude-opus-4-8` |
| `codex` | OpenAI Codex | `codex exec --sandbox workspace-write --ask-for-approval never --ignore-user-config --json` |
| `kimi` | Anthropic-compatible | same `claude` CLI, `ANTHROPIC_BASE_URL=https://api.moonshot.ai/anthropic` `ANTHROPIC_MODEL=kimi-k2.7-code` |

**Claude is the default** — a job that names no agent is Claude, both at the type level and on the
wire (`#[serde(default)]`), so existing App frames keep working unchanged. Select per-job:

```bash
fxrun agents                          # list backends + their headless invocation
fxrun route review --agent codex      # → kernel=atc agent=codex
WEAVE_FXRUN_AGENT=kimi fxrun route agent
                                       # → kernel=atc agent=kimi agent_source=weave
FXRUN_AGENT=codex fxrun route agent    # legacy local fallback when Weave is absent
```

Precedence is explicit signed job/`--agent` > `WEAVE_FXRUN_AGENT` > legacy `FXRUN_AGENT` > Claude.
The selector lives on the signed `JobSpec` when supplied by the front door, so an explicit agent is
integrity-protected end to end.

## Status

Implemented and tested: the signed job-spec contract + signature verification (S7), the kernel
router (delegate map), the fork-PR isolation policy, and the JIT lifecycle state machine.

The Actions supervisor is live: `fxrun-actions` can install the upstream GitHub Actions runner,
mint short-lived registration tokens with `gh`, register the canonical FlexNetOS org-scoped runner
by default, run one ephemeral job, or install a persistent service. Repo-scoped runner registration
is an explicit sandbox/exception only — never the default for `envctl`, `meta`, or any other peer.
Install enforces GitHub's mandated minimum runner version (`≥ 2.329.0`, changelog 2026-06-12) —
below it GitHub refuses registration / pauses job queuing and the runner is exposed to the
Runner-Escape host-secret leak, so the supervisor fails closed on a stale pin. The UDS dispatch +
kernel invocation, envctl-style secret injection, and provenance gates are wired seams. Current
hardening adds full-envelope authority signatures, private UDS socket binding, nonce-based fresh
workspaces, pre-extract runner SHA-256 verification, and CI `cargo audit`; the remaining backlog
covers registration-token argv removal, rate-limit clock freshness, structured kernel result/status,
and desktop approval/re-entry. The confirmed P3 recipe for first-party artifacts remains GitHub
Artifact Attestations (`actions/attest-build-provenance@v3`, SLSA Build L2 via OIDC + Sigstore),
verified with `cosign verify-attestation` / `slsa-verifier`.

Canonical operation is one org-scoped FlexNetOS runner, shared by meta peer repositories through the
labels `self-hosted,linux,x64,local,flexnetos`. A local `.runner` that points at
`https://github.com/FlexNetOS/<repo>` is scope drift, not a new default. Strict upgrade path: stand
up the org-scoped runner in a clean `RUNNER_HOME`, verify the shared labels service the required
meta peers, then retire the old repo-scoped service/config. Do not mutate a live repo-scoped runner
home in place, and do not create a repo-scoped `envctl`/`meta` runner as a special-case fix.

On this workstation the persistent FlexNetOS org runners live inside this repo-local committed
operations root, `_work/`. The default `fxrun-actions` paths target slot `actions-runner-01`; the
second parallel slot uses the same layout with suffix `-02`. Historical `/home/drdave/_work` runner
state is retired into `_work/archives/retired/`; do not move active runner state back outside this
repo-local `_work` tree.

### Org runner-group dispatch repair

Runner dispatch depends on GitHub's org runner-group repository access, not only local runner
registration. The normal user `gh` token may lack `admin:org`, so org-runner inspection/repair uses
envctl's GitHub App token minted by `secretctl` instead of asking operators to re-authenticate `gh`.
The script resolves `secretctl` from `FXRUN_SECRETCTL`, `PATH`, or a discovered `META_ROOT`.

```bash
scripts/repair-org-runner-group.sh          # dry-run + evidence under _work/org-runner-repair/
scripts/repair-org-runner-group.sh --apply  # add missing active FlexNetOS repos to the group
```

The script is strict-upgrade only: it repairs selected runner-group repository membership and does
not remove runners, downgrade runner binaries, or mutate healthy runner services. If dispatch still
fails after membership is correct, re-register into a clean replacement runner home and prove parity
before retiring existing service state.

## Build

```bash
cargo build --workspace
cargo test  --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
fxrun doctor
```

## Local Ubuntu release

`flexnetos_runner` owns the local release lane for this workstation. The first supported target is
Ubuntu 26.04 on `x86_64`, with artifacts written to the workspace-level `release/` directory.

```bash
FXRUN_CARGO=/home/flexnetos/FlexNetOS/src/flexnetos_runner/_work/runner-home-02/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo \
  scripts/build-local-ubuntu-release.sh
```

The default release builds `flexnetos_runner`, `meta`, and `yazelix` from local source and stages
Yazelix runtime assets alongside provenance and SHA-256 manifests. See
[`docs/local-ubuntu-release.md`](docs/local-ubuntu-release.md).


## Live runner evaluation

Use `scripts/eval-runners.sh` to evaluate both repo-local FlexNetOS org runner slots in real time.
The tool dispatches the committed `runner-smoke.yml` workflow once per slot, isolates the target by
stopping its peer by default, streams run status while it waits, and restores both services on exit.

```bash
scripts/eval-runners.sh
# optional: scripts/eval-runners.sh --no-isolate --poll-secs 2 --timeout-secs 600
```

The evaluator writes a timestamped proof bundle under `_work/evals/<timestamp>/`:

| Artifact | Purpose |
|---|---|
| `summary.md` | Human scorecard with per-runner conclusion, accuracy, timing table, task output, failures, and lessons learned. |
| `metrics.jsonl` | Machine-readable one-record-per-runner metrics including dispatch-to-visible, dispatch-to-created, pickup latency, execution time, total turnaround, step durations, assertions, output, failures, and lessons. |
| `api-*.json` | Before/after GitHub org runner API snapshots for online/busy/label state. |
| `run-*.json` / `run-*.log` | GitHub run metadata plus raw job log output used for accuracy checks. |
| `journal-*.log` / `diag-*.log` | Local systemd and runner diagnostic tails around the probe. |

A result is accurate only when the workflow succeeds, the log reports the expected runner name, and
`RUNNER_WORKSPACE` is under that slot's repo-local `_work/actions-runner-0N-work/` directory. The
pickup-latency metric is the GitHub run creation time to job start time; total turnaround is local
dispatch to job completion.

## Safety posture

Untrusted **fork PRs never run on self-hosted hardware** (routed to GitHub-hosted/sandboxed);
ephemeral JIT runners (one job, then destroyed); non-root, no Docker socket; secrets reach a child
only via an envctl relay-bearer (the real key never enters the child env). See ADR-0008 §2/§6.
