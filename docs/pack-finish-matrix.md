# Pack Finish Matrix

This matrix surfaces the quarantined production pack as a map only. A row
advances only when a live surface proves it: clean `env -i` commands, envctl
tables, GitKB/meta status, Yazelix doctor, runner provenance, release checksums,
or clean-prefix install logs.

## Source Boundary

| Surface | Role | Path |
| --- | --- | --- |
| Quarantine manifest | Explains why the old pack is read-only context | `/home/flexnetos/FlexNetOS/_quarantine/20260630T234500Z/README.md` |
| Quarantined pack | Historical execution map, not proof | `/home/flexnetos/FlexNetOS/artifacts/recovery/old-pack-context/flexnetos_production_execution_pack` |
| Runner lane | Current release and provenance owner | `/home/flexnetos/meta/flexnetos_runner` |
| Workspace ledgers | Host-local proof and remaining gaps | `/home/flexnetos/FlexNetOS/WORKLOG.md`, `/home/flexnetos/FlexNetOS/LOCAL_WORKAROUNDS.md`, `/home/flexnetos/FlexNetOS/COMMAND_LEDGER.csv` |
| Release output | Built artifacts, manifests, checksums, provenance | `/home/flexnetos/FlexNetOS/release` |

Use this clean-shell prefix when proving rows from the host:

```sh
env -i HOME=/home/flexnetos USER=flexnetos LOGNAME=flexnetos PATH=/home/flexnetos/FlexNetOS/usr/bin:/home/flexnetos/.local/bin:/home/flexnetos/.nix-profile/bin:/run/current-system/sw/bin:/usr/bin:/bin
```

## Live Matrix

| Pack lane | Current owner | Live state | Required proof | Next action |
| --- | --- | --- | --- | --- |
| Quarantine doctrine | Workspace root and runner docs | Surfaced here; pack remains read-only context | This doc plus quarantine README | Keep pack artifacts out of completion gates |
| Meta project graph | `src/meta` | Healthy in clean shell | `git-kb verify --json`; `meta git status --short --sequential` | Preserve no-ahead/no-behind before release |
| GitKB memory and tasks | Meta root, `flexnetos_runner` | Meta KB verifies; runner task `019f2942-b2c9-75d3-9165-fa064437f69e` tracks this matrix | GitKB task/document commits in runner; meta verify | Commit this matrix through runner GitKB and keep task state current |
| Yazelix foundation runtime | `src/yazelix` and installed Yazelix profile | Healthy in clean shell | `yzx status --json`; `yzx doctor` | Keep runtime doctor green after release staging |
| Codex clean runtime | Active host Codex owner and Yazelix foundation package | Aligned: clean shell, compatibility link, standalone app-server pointer, and running daemon report `0.143.0-alpha.35` | `command -v codex`; `codex --version`; `codex app-server daemon version`; `codex doctor --all`; archive receipts | Watch for update-manager drift back to standalone stable |
| envctl tables | `src/envctl` | Aligned: release binary is exposed at workspace `usr/bin/envctl`; clean-shell catalog tables and render pass | `command -v envctl`; `envctl catalog tables`; `envctl catalog render`; generated catalog output path | Keep this frontdoor until the runner bundle installs the packaged envctl binary |
| RTK and raw logs | Workspace RTK policy and runner scripts | Policy surface exists; release failures must preserve raw logs | Raw log path in command ledger or runner provenance | Ensure runner release gates tee failing command output to raw logs |
| Runner release gate | `flexnetos_runner` | Output root alignment fixed; dirty runner state classified and archived; full release still blocked by pre-existing generated state and partial release outputs | Runner git status snapshot; dirty inventory archive; `fxrun doctor`; local release build log | Decide owner handling for dirty source edits, then build the full catalog from a clean release surface |
| Release manifest and checksums | Runner release pipeline | Partial artifacts exist; no complete v0.1 bundle is proven | Complete manifest, BOM/provenance, and checksum files under canonical release output | Build the full catalog into canonical release output and checksum every artifact |
| Clean-prefix install | Runner installer | Not complete for the current release candidate | `PREFIX=<clean dir>` install log; command ledger entry; installed binary smoke tests | Install from the built bundle into a clean prefix and log every command |
| Login/runtime smoke | Workspace host session | Not proven for the current release candidate | Clean login or session smoke transcript with active paths and versions | Run only after release bundle and clean-prefix gates pass |
| Handoff and v0.2 backlog | Runner docs and GitKB | This matrix is the live v0.1 finish map | Handoff doc, GitKB state, release notes | Move only non-v0.1 items to explicit backlog after live proof |

## Finish Queue

1. Decide owner handling for pre-existing runner edits
   (`.codex/hooks.json` and `release/catalog.tsv`) without reverting user work.
2. Build the full runner catalog, then generate release manifests, provenance,
   and checksums from the canonical output.
3. Run a clean-prefix install from the release bundle and record the install log.
4. Run final clean login/runtime smoke only after the release and clean-prefix
   gates are green.

## Advancement Rule

A matrix row can move to done only with a timestamped command, exit status, and
evidence path. Pack files, stale task tables, or recovered artifacts may explain
why a row exists, but they cannot satisfy the row by themselves.

## Proof Captured

| Timestamp UTC | Row | Command or surface | Result |
| --- | --- | --- | --- |
| 2026-07-03T18:35:57Z | Runner release gate | `env -i ... scripts/build-local-ubuntu-release.sh --help` | Help now reports default `FXRUN_RELEASE_DIR` as `$FXRUN_WORKSPACE_ROOT/release` |
| 2026-07-03T18:36:12Z | Runner release gate | `env -i ... FXRUN_RELEASE_ALLOW_HOST_MISMATCH=1 FXRUN_CARGO=... FXRUN_RUNNER_HOME=/tmp/fxrun-check-runner-home scripts/build-local-ubuntu-release.sh --check-only` | Catalog validation passed for all 15 rows |
| 2026-07-03T18:36:05Z | Codex clean runtime | `env -i ... command -v codex; codex --version` | Drift found: clean shell resolved `/home/flexnetos/.local/bin/codex`, version `0.142.5` |
| 2026-07-03T18:36:58Z | Codex clean runtime | Archive and relink `/home/flexnetos/.local/bin/codex` | Compatibility link restored to `/home/flexnetos/.nix-profile/bin/codex`; archive sha256 `1f39f9fdf1eec4b830eca1ea86e5ee0d5e3eac153077958432a711053c8e50c0` |
| 2026-07-03T18:37:43Z | Codex clean runtime | Archive and relink `/home/flexnetos/.codex/packages/standalone/current` | Managed app-server pointer restored to `0.143.0-alpha.35`; archive sha256 `d1e088fca402c482e6fe0c3f27b02e7c50ec29aa15805d7ad96247117697ba5f` |
| 2026-07-03T18:39:45Z | Codex clean runtime | `env -i ... codex app-server daemon start` | Started pid-managed app-server with managed, CLI, and app-server versions all `0.143.0-alpha.35` |
| 2026-07-03T18:39:55Z | Codex clean runtime | `env -i ... codex doctor --all` | Passed: `18 ok`, `1 notes`, `0 warn`, `0 fail`; app-server version `0.143.0-alpha.35` |
| 2026-07-03T18:36:05Z | envctl tables | `env -i ... command -v envctl` | No clean-shell `envctl` frontdoor found |
| 2026-07-03T18:41:46Z | envctl tables | `env -i ... cargo build -p envctl --release --locked` | Release build passed from `src/envctl` using the runner-local Rust toolchain |
| 2026-07-03T18:41:54Z | envctl tables | Workspace frontdoor update | `/home/flexnetos/FlexNetOS/usr/bin/envctl` now points at `/home/flexnetos/FlexNetOS/src/envctl/target/release/envctl` |
| 2026-07-03T18:42:00Z | envctl tables | `env -i ... command -v envctl; envctl --version` | Clean shell resolves `/home/flexnetos/FlexNetOS/usr/bin/envctl`; version `envctl 0.1.0` |
| 2026-07-03T18:42:09Z | envctl tables | `env -i ... envctl catalog tables --repo-root /home/flexnetos/FlexNetOS/src/envctl` | Passed with 11 tables, including `components=97`, `settings=4925`, `codedb_file_imports=3549`, `observed_facts=699` |
| 2026-07-03T18:44:06Z | envctl tables | `env -i ... envctl catalog render --repo-root /home/flexnetos/FlexNetOS/src/envctl --out /home/flexnetos/FlexNetOS/var/tmp/envctl-catalog-render-20260703T1844Z --target-root /home/flexnetos/FlexNetOS` | Passed: `generated_files=41`, `generated_config_rows=41`, `bytes=204790943`, `mutating_repo=no` |
| 2026-07-03T18:45:49Z | Runner release gate | Dirty runner inventory archive | `/home/flexnetos/FlexNetOS/var/lib/codex-runtime-gate/archives/flexnetos-runner-dirty-inventory-20260703T1845Z.tar.gz`, sha256 `65967f5ab577eab5ab260c88c37305f2838da2752a7fa83ff81528ba2beab7ba`; `_work=87G`, `release/out=1.9G` |
