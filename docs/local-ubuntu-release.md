# Local Ubuntu Release

`flexnetos_runner` is the release host for the local FlexNetOS bundle. The
release output surface is the workspace-level `release/` directory:

```text
/home/flexnetos/FlexNetOS/release/
```

See [pack-finish-matrix.md](pack-finish-matrix.md) for the live finish matrix
derived from the quarantined production pack. The pack is intent and ordering;
release readiness comes only from live proof surfaces such as clean `env -i`
commands, envctl tables, GitKB/meta status, Yazelix doctor, runner provenance,
release checksums, and clean-prefix install logs.

The first supported target is intentionally narrow:

- OS: Ubuntu 26.04
- architecture: x86_64
- build host: this local workstation

The release lane reads the component catalog at
[`release/catalog.tsv`](../release/catalog.tsv), then stages a single archive
with provenance. The first-class entry point is `fxrun release`, which pre-wires
a runner-local `cargo` and `bun` so no `FXRUN_CARGO=` prefix is required:

```bash
fxrun release check    # validate host/toolchain/catalog (script --check-only)
fxrun release build    # compile, run the proof gate, stage provenance, write the tarball
```

The workspace root — and therefore the `<root>/release` output directory — is
resolved deterministically from the script's own on-disk location, so it lands in
`/home/flexnetos/meta/release` even if the historical `/home/flexnetos/FlexNetOS`
symlink is absent. `FXRUN_WORKSPACE_ROOT` / `FLEXNETOS_ROOT` / `FXRUN_RELEASE_DIR`
still override. The underlying script stays usable standalone:

```bash
FXRUN_CARGO=.../stable-x86_64-unknown-linux-gnu/bin/cargo \
  scripts/build-local-ubuntu-release.sh
```

### Runner-proof gate

Before the tarball is written, `fxrun release build` runs
`codedb export runner_proof_manifest --repo-id flexnetos_runner --repo-path
<root>/src/flexnetos_runner/crates --format json` and enforces:

- `status == failed` → always blocks the build.
- `status == pending` → blocks unless the `gate_id` is in `FXRUN_PROOF_PENDING_ALLOW`.
- `status == degraded` → blocks unless the `gate_id` is in `FXRUN_PROOF_DEGRADED_ALLOW`
  **and** the row names a `raw_log_path`.

The current `runner_proof_manifest` emits permanent, owned deferrals that are the
documented default exceptions:

| gate_id | status | reason |
|---|---|---|
| `release_readiness` | pending | `runner_owner=true`; closed by the staged proof receipt |
| `fixture_matrix` | pending | CodeDB-side future fixture-matrix work |
| `generated_artifact_reproduction` | pending | CodeDB-side future reproduction-mode work |
| `capture_gaps_recorded` | degraded | raw log `logs/CDB039-runner.log` |

The scan targets `crates/` (the runner's Rust source), not the repo root: the
committed `_work/` tree carries vendored rustup toolchain sources that CodeDB's
parser rejects, which would otherwise crash the gate. The manifest
(`runner_proof_manifest.json`) and a `requirement-proof-receipt.txt` are written
into the staged `provenance/` so the tarball carries its own local proof. If
CodeDB (or `python3`) is unavailable, or the export fails, the gate no-ops with a
clear skip and records the reason in the receipt, so the lane still builds
standalone.

The catalog is the source of truth. It currently includes:

- `flexnetos_runner`
- `meta`
- `meta-agent`
- `gitkb`
- `codex`
- `envctl`
- `beads_rust`
- `rtk-tokenkill`
- `yazelix`
- `yazelix-helix`
- `nu_plugin`
- `loop_lib`
- `meta_plugin_protocol`
- `bun`

Cargo catalog rows build from local source. `copy-bin` rows stage existing
workspace-owned binary payloads such as GitKB, Codex, and Bun until their source
repos are promoted into the peer catalog. Yazelix is included by building
`src/yazelix/rust_core` and staging the runtime assets needed by the generated
package surface. The bundle does not use a Nix store path as its payload source;
it copies locally built binaries and local repository assets into the release
stage.

Outputs:

- `release/flexnetos-ubuntu-26.04-x86_64-<timestamp>.tar.gz`
- `release/flexnetos-ubuntu-26.04-x86_64-<timestamp>.tar.gz.sha256`
- `release/staging/flexnetos-ubuntu-26.04-x86_64-<timestamp>/provenance/`

Use `--check-only` to validate host and toolchain wiring without compiling:

```bash
scripts/build-local-ubuntu-release.sh --check-only
```

The script accepts `FXRUN_RELEASE_COMPONENTS` for explicit component selection;
unset means every non-comment row in `release/catalog.tsv`.
