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
with provenance:

```bash
FXRUN_CARGO=/home/flexnetos/FlexNetOS/src/flexnetos_runner/_work/runner-home-02/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo \
  scripts/build-local-ubuntu-release.sh
```

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
