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

## Workspace

| Crate | Bin | Role |
|-------|-----|------|
| `runner-core` | — | Pure core: signed job-spec type, kernel router (delegate-only), fork-PR isolation policy, JIT lifecycle state. Fully unit-tested. |
| `runner-actions` | `fxrun-actions` | Self-hosted Actions runner supervisor (JIT register → run one → deregister). P1. |
| `runner-dispatch` | `fxrun-dispatch` | UDS server: verify signed job spec → route → invoke kernel. P2. |
| `runner-cli` | `fxrun` | Operator CLI: `route`, `doctor`, `status`. |

## Status

Implemented and tested: the signed job-spec contract + signature verification (S7), the kernel
router (delegate map), the fork-PR isolation policy, and the JIT lifecycle state machine.

The Actions supervisor is live: `fxrun-actions` can install the upstream GitHub Actions runner,
mint short-lived registration tokens with `gh`, register repo/org-scoped runners, run one
ephemeral job, or install a persistent service. The UDS dispatch + kernel invocation (P2), envctl
secret injection, and SLSA/cosign provenance (P3) remain fail-closed typed seams.

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
