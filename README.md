# flexnetos_runner

The **execution plane** of FlexNetOS's GitHub↔local automation: a local, self-hosted runner that
executes CI/jobs/loops on the developer's own hardware and **connects all of meta** by routing work
to the existing kernels rather than reimplementing them. It is the muscle paired with
[`flexnetos_github_app`](https://github.com/FlexNetOS/flexnetos_github_app)'s control plane.

Design: **ADR-0008** (`handoff/docs/adr-0008-flexnetos-app-runner.md`).

Two shapes, by design:
1. **Self-hosted GitHub Actions runner** — the canonical hermetic flake under
   [`nix/gha-runner`](nix/gha-runner/), with a pinned `github-runner` substrate, brokered
   registration, volatile profile-runtime state, and an explicit foreground session.
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
| `runner-dispatch` | `fxrun-dispatch` | UDS server: verify signed job spec → route → invoke kernel. P2. |
| `runner-cli` | `fxrun` | Operator CLI: `route`, `agents`, `release`, `doctor`. |
| `nix/gha-runner` | `nix run .#start` | Canonical GitHub Actions substrate and Metaharness layer. |

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
router (delegate map), fork-PR isolation policy, UDS dispatch, bounded kernel invocation, and the
canonical Nix GitHub runner composition.

The Actions worker is `nix/gha-runner`: nixpkgs supplies the pinned upstream runner, envctl is the
sole secret/token minter, and the launcher registers one org-scoped runner named `flexnetos-nix`
with labels `self-hosted,flexnetos,nix`. Mutable runner state lives only under
`$XDG_RUNTIME_DIR/yazelix/profile-runtime/gha-runner`.

`NO_SYSTEM_DEPTHS` is a hard rule. The Nix store is passive, so this repo deliberately provides no
unattended reboot activation. Operators explicitly run `nix run .#start` from `nix/gha-runner`;
the listener stays in the foreground, reuses valid state in the current boot, and re-mints plus
re-registers after volatile state disappears. See
[`nix/gha-runner/RUNBOOK.md`](nix/gha-runner/RUNBOOK.md).

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

## Templates

Reusable fleet templates live under [`templates/`](templates/). The first
template is [`templates/git-upstream-worktree-sync/`](templates/git-upstream-worktree-sync/),
which packages the safe upstream-sync pattern proven in `rtk-tokenkill`: create
an isolated `.worktrees/<sync-branch>` checkout, fetch/merge the upstream
remote-tracking branch there, and only land the result directly when local
branch policy allows.

## Local Ubuntu release

`flexnetos_runner` owns the local release lane for this workstation. The first supported target is
Ubuntu 26.04 on `x86_64`, with artifacts written to the workspace-level `release/` directory. The
LOCAL compile lane is first-class through `fxrun release` — it resolves a runner-local `cargo` and
`bun` automatically, so no `FXRUN_CARGO=` prefix is required:

```bash
fxrun release check      # host + toolchain + catalog validation (script --check-only)
fxrun release build      # compile the catalog, stage provenance + proof, write the tarball
# scope to present components, override output, etc.:
fxrun release check --components flexnetos_runner,envctl,rtk-tokenkill
fxrun release build --out /tmp/relout --cargo /path/to/cargo
```

The output root resolves deterministically to `<workspace-root>/release` from the script's own
location (symlink-independent); `FXRUN_RELEASE_DIR` still overrides it. The underlying script stays
usable standalone:

```bash
FXRUN_CARGO=.../stable-x86_64-unknown-linux-gnu/bin/cargo \
  scripts/build-local-ubuntu-release.sh
```

**Runner-proof gate.** Before staging the tarball, the build runs
`codedb export runner_proof_manifest` and fails closed on any `failed` status, any un-allowlisted
`pending`, or any `degraded` row missing a raw-log reference. The manifest carries permanent
runner-owned deferrals (`release_readiness`, `fixture_matrix`, `generated_artifact_reproduction`,
and the `capture_gaps_recorded` degradation), which are the documented default exceptions
(`FXRUN_PROOF_PENDING_ALLOW` / `FXRUN_PROOF_DEGRADED_ALLOW`). The gate scans the runner's Rust
source tree (`crates/`, not the `_work/`-laden repo root, whose vendored toolchain sources crash
CodeDB's parser). The manifest and a `requirement-proof-receipt.txt` are staged into the tarball's
`provenance/`. When CodeDB is unavailable the gate no-ops with a clear skip so the lane still builds
standalone.

The release reads [`release/catalog.tsv`](release/catalog.tsv) as the component source of truth.
That catalog includes the Rust workspaces, GitKB/Codex binary payloads, Yazelix, envctl,
`flexnetos_runner`, meta peers, and workspace tool frontdoors needed for the portable local state.
See
[`docs/local-ubuntu-release.md`](docs/local-ubuntu-release.md).


## Live runner evaluation

Start the canonical foreground listener, then dispatch `.github/workflows/runner-smoke.yml` at the
branch under test. Completion evidence is the GitHub run ID/URL, head SHA, successful conclusion,
runner name, and the 33/33 offline composition gate. Exact commands are in the Nix runbook.

## Safety posture

Untrusted **fork PRs never run on self-hosted hardware** (routed to GitHub-hosted/sandboxed).
The local listener is non-root, has no Docker socket, runs from an immutable Nix closure, and keeps
mutable state in volatile profile runtime. The App private key remains inside envctl; only
short-lived opaque tokens cross the broker boundary. See ADR-0008 §2/§6.
