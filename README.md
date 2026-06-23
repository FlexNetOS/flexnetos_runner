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
- **Planned:** signed full-envelope provenance, UDS/socket hardening, fresh workspace by construction,
  artifact verification, structured kernel result/status, desktop approval/re-arm flow.

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
mint short-lived registration tokens with `gh`, register repo/org-scoped runners, run one
ephemeral job, or install a persistent service. Install enforces GitHub's mandated minimum runner
version (`≥ 2.329.0`, changelog 2026-06-12) — below it GitHub refuses registration / pauses job
queuing and the runner is exposed to the Runner-Escape host-secret leak, so the supervisor fails
closed on a stale pin. The UDS dispatch + kernel invocation, envctl-style secret injection, and
provenance gates are wired seams; the fresh backlog now hardens full-envelope provenance, socket
permissions, workspace freshness, supply-chain verification, and structured result status; the confirmed P3 recipe is GitHub Artifact
Attestations (`actions/attest-build-provenance@v3`, SLSA Build L2 via OIDC + Sigstore), verified
with `cosign verify-attestation` / `slsa-verifier`.

## Build

```bash
cargo build --workspace
cargo test  --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
fxrun doctor
```

## Safety posture

Untrusted **fork PRs never run on self-hosted hardware** (routed to GitHub-hosted/sandboxed);
ephemeral JIT runners (one job, then destroyed); non-root, no Docker socket; secrets reach a child
only via an envctl relay-bearer (the real key never enters the child env). See ADR-0008 §2/§6.
