---
id: 019f2503-2a55-78f0-a491-6f1f0e571d85
slug: tasks/cache-command-automation-seams
title: "Automate cache commands with full seams"
type: task
status: completed
priority: high
tags: [runner, cache, compression, commands, triggers, connectors, hooks, contracts, gates, tests, guardrails]
---

## Parent / related task
References `tasks/cache-compression-automation`.

## Intent
Turn the cache-compression design into three automated command seams with no hidden/manual-only gap:

1. `fxrun cache audit`
2. `fxrun cache compress`
3. `fxrun cache restore`

Each command must have explicit triggers, connectors, hooks, contracts, gates, tests, guardrails, and rollback behavior.

## Command 1: `fxrun cache audit`

### Purpose
Inventory cache pressure and decide what is safe to compress without mutating anything.

### Inputs / flags
- `--root <path>`: cache root, default repo `_work/`
- `--slot <01|02|all>`: runner slot selector
- `--min-age <duration>`: default `7d`
- `--format <human|json>`: default human, CI uses json
- `--manifest-out <path>`: optional audit manifest path
- `--include <glob>` and `--exclude <glob>`: additive allow/deny patterns

### Connectors
- Local filesystem scan connector for `_work/runner-home-*`, `_work/actions-runner-*-work`, `_work/repos/actions-runner-*`
- Runner process connector using `/proc`, `pgrep`, and optional `lsof`/`fuser`
- Git connector to identify tracked/untracked/cache paths and avoid `.git/objects`
- GitKB connector to write/refresh evidence against this task when requested later
- JSON report connector for CI, hooks, and dashboards

### Triggers
- Manual: `fxrun cache audit --format json`
- Pre-compression gate before every `fxrun cache compress`
- Scheduled systemd timer dry-run before compression windows
- GitHub Actions workflow dispatch for storage-pressure diagnostics
- Post-runner-eval hook to attach cache pressure stats to runner evidence

### Hooks
- PreToolUse/local hook may run audit in dry mode before destructive cache operations
- PostToolUse/local hook can record bytes-before/after into `_work/cache-compression/manifests/`
- Git pre-commit guard warns if huge new cache files are added without an audit manifest
- Optional systemd `ExecCondition` runs audit and blocks compression if active runner workers exist

### Contract
- Pure read-only by default
- JSON schema includes: candidate path, original size, mtime, reason, selected compressor, skip reason, active-process evidence, tracked-state evidence
- Exit codes:
  - `0`: audit complete, candidates may exist
  - `1`: invalid arguments/config
  - `2`: unsafe active runner/cache state detected
  - `3`: scan/read error

### Gates/tests
- Unit tests for age threshold, include/exclude, denylist, active-process skip, JSON schema
- Fixture tests over sample `_work` trees
- Gate: `fxrun cache audit --root <fixture> --format json` must validate against schema
- Gate: audit must never mark `.runner`, credentials, `.git`, service unit files, or live runner workers compressible

## Command 2: `fxrun cache compress`

### Purpose
Compress old safe cache files in place, atomically, with manifest-backed restore.

### Inputs / flags
- `--root <path>`: default `_work/`
- `--slot <01|02|all>`
- `--min-age <duration>`: default `7d`
- `--dry-run`
- `--compressor <zstd|gzip>`: default `zstd`
- `--level <n>`: bounded compressor level
- `--manifest <path>`: output manifest, default `_work/cache-compression/manifests/<timestamp>.json`
- `--lock <path>`: default per root/slot lock
- `--max-bytes <n>` and `--max-files <n>`: safety caps
- `--restore-stub <off|manifest|stub>`: default manifest-only

### Connectors
- Calls `fxrun cache audit` internally and consumes its JSON contract
- Compressor connector: `zstd` preferred; fallback to gzip only if configured and tested
- File lock connector to prevent concurrent per-slot compression
- Process connector to re-check active workers immediately before compressing each file
- Manifest connector for byte/mtime/mode/sha256 restore metadata
- GitHub Actions connector for dispatchable storage-maintenance workflow
- systemd connector for idle-window timer/service deployment

### Triggers
- Manual: `fxrun cache compress --dry-run` then without dry-run
- systemd timer after runner idle window
- GitHub Actions workflow_dispatch: `cache-maintenance.yml`
- Optional post-merge/post-CI idle hook when no local runner jobs are active
- Storage-pressure trigger when `_work` exceeds configured byte threshold

### Hooks
- Pre-compress hook: audit + active runner/process gate
- Per-file pre-write hook: denylist + open-file recheck
- Per-file post-write hook: decompression checksum verification
- Post-compress hook: stats report + optional GitKB evidence note
- Failure hook: keep original, remove tmp, mark manifest entry failed

### Contract
- Never mutates files unless audit selects them and second safety check passes
- Atomic algorithm:
  1. lock root/slot
  2. write `<path>.zst.tmp`
  3. fsync compressed temp
  4. verify decompressed bytes hash equals original sha256
  5. write/append manifest entry
  6. rename temp to `<path>.zst`
  7. remove original only after manifest and compressed file verify
- Idempotent: skip existing compressed files and manifest-known entries
- JSON output includes skipped/failed/compressed counts and bytes saved
- Exit codes:
  - `0`: completed successfully
  - `1`: invalid args/config
  - `2`: blocked by active runner/process gate
  - `3`: verification failure, original preserved
  - `4`: partial compression with recoverable failures recorded

### Gates/tests
- Red test first: `.runner`, `.credentials`, `.git/**`, active `Runner.Worker` paths, and files younger than threshold are never compressed
- Unit tests for atomic temp/rename cleanup and checksum verification
- Integration fixture: compress old cache files, verify originals removed only after `.zst` validates
- Idempotency test: second run changes nothing
- Concurrency test: two compress runs on same slot honor lock
- CI gate: `cargo test -p runner-cli cache_compression`
- Script gate: `bash -n scripts/cache-maintenance.sh` if wrapper exists

## Command 3: `fxrun cache restore`

### Purpose
Restore compressed cache files from manifests or direct `.zst` paths with byte-for-byte fidelity.

### Inputs / flags
- `fxrun cache restore <path>`: restore one file or compressed path
- `--manifest <path>`: restore from manifest
- `--all --root <path>`: restore all known files under a root
- `--dry-run`
- `--overwrite <never|if-missing|always>`: default `if-missing`
- `--verify-only`: verify compressed file and manifest without restoring

### Connectors
- Manifest connector to resolve original path and metadata
- Decompressor connector using the compressor recorded in manifest
- Filesystem connector to restore mode/mtime and verify sha256
- Git connector to report whether restored paths are tracked, modified, or ignored
- Incident connector: failed restore can create/append GitKB incident evidence in a follow-up command

### Triggers
- Manual emergency restore
- Pre-job hook if a tool expects an uncompressed file and only compressed form exists
- CI restore round-trip tests
- Post-compression verify-only trigger
- Rollback trigger if a compression run reports partial failure

### Hooks
- Pre-restore hook checks target path safety and overwrite policy
- Post-restore hook verifies sha256/mode/mtime and updates manifest state
- Failure hook preserves compressed artifact and writes recovery instructions

### Contract
- Restore must be lossless for bytes, mode, mtime, and original relative path
- Never overwrites newer files unless `--overwrite always`
- JSON output includes restored files, skipped files, verification results, and errors
- Exit codes:
  - `0`: restored/verified successfully
  - `1`: invalid args/config
  - `2`: overwrite policy blocked restore
  - `3`: decompression/checksum failure
  - `4`: partial restore with manifest-recorded failures

### Gates/tests
- Round-trip test: compress then restore equals original bytes/mode/mtime
- Manifest-only restore test
- Direct `.zst` restore test
- Overwrite-policy tests
- Corrupt compressed file test must fail closed and preserve artifacts

## Cross-cutting seams that must not be missed

### Configuration seam
- Add a checked-in config file or documented defaults for roots, age, denylist, compressor, caps, lock path, and manifest path.
- Environment overrides must be explicit: `FXRUN_CACHE_*` only.

### Security/secret seam
- Denylist credentials and live runner identity files.
- Do not print secrets from paths or file content.
- Manifests store hashes/metadata only, never file contents.

### Portability seam
- No hard-coded `/home/flexnetos` in core logic.
- Prefix/root passed through CLI and service wrappers.
- Works in released install location and repo checkout.

### Runner-liveness seam
- Must detect active `Runner.Listener` / `Runner.Worker` and active workspace paths.
- Compression should run only in idle windows unless targeting files outside active roots.

### Git/tracking seam
- Preserve tracked cache artifacts instead of hiding them.
- `.gitignore` is not the feature boundary.
- Git status impact must be reported after compression/restore.

### Archive seam
- Archives under `_work/archives` are not recompressed unless explicitly included.
- Retired `_work` archives may be audited separately but are not default targets.

### Observability seam
- Every run emits human summary and JSON report.
- Reports include before/after bytes, compression ratio, skips, errors, and safety gates.

### CI/GitHub seam
- Add workflow_dispatch maintenance workflow.
- CI must run unit/integration dry-run tests without touching live runner state.
- PR checks must fail if unsafe paths become compressible.

### Docs seam
- Add docs for command usage, defaults, safety model, restore procedure, and emergency rollback.

### Release seam
- Include commands in released `fxrun` binary and installation docs.
- Verify installed-location behavior with fixture prefix.

## Acceptance criteria
- All three commands exist and expose `--help`.
- All three commands support JSON output for automation.
- Compression cannot touch denied/live paths under tests or live audit.
- Restore is byte-for-byte verified.
- Workflow/systemd trigger examples are checked in and tested with dry-run.
- A strict gate verifies hooks, contracts, docs, and tests for every seam listed above.

## Implementation evidence
- Added top-level `fxrun cache` command surface with `audit`, `compress`, and `restore` subcommands.
- Added zstd-backed in-place compression with `.zst.tmp` atomic write, decompression checksum verification, manifest metadata, and original removal only after verification.
- Added manifest/direct restore with checksum verification, overwrite policy, mode/mtime restore, dry-run, and verify-only modes.
- Added denylist and liveness guardrails for `.runner`, credentials, service/config identity files, `.git`, `_work/archives`, active paths, young files, and open files when `lsof` is available.
- Added JSON/human output contracts and command help for all three commands.
- Added `scripts/cache-maintenance.sh`, `.github/workflows/cache-maintenance.yml`, and `systemd/user/flexnetos-cache-maintenance.{service,timer}` trigger examples.
- Added `docs/cache-compression.md` with usage, defaults, safety model, restore procedure, wrapper env vars, and release gate.
- Validation evidence: red tests were created first; then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace --all-features`, `bash -n scripts/cache-maintenance.sh`, cache command help checks, cache audit JSON smoke, and forge-loop strict audits all passed locally.
