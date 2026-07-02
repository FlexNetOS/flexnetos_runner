---
id: 019f24ef-9b0c-7f42-9dab-856a53e35309
slug: tasks/cache-compression-automation
title: "Design cache compression automation"
type: task
status: completed
priority: high
tags: [runner, cache, compression, storage, automation]
---

## Problem
Tracked runner/cache state is intentionally preserved, but active runner homes and `_work` trees accumulate high-churn cache files. Large cache databases, event logs, tool caches, and old envctl/kache/cargo artifacts make the repo and runner storage expensive while most old entries are cold.

## Goal
Design and implement a cache compression automation that compresses older cache files in place so they remain discoverable and accessible, but consume less disk and repository space.

## Proposed design
- Add a repo-local automation command, e.g. `fxrun cache compress`, plus a script wrapper for Actions/systemd use.
- Scan configured cache roots under `_work/runner-home-*` and `_work/actions-runner-*-work` using a conservative allowlist:
  - kache event logs and old SQLite/WAL snapshots
  - envctl cache state/log artifacts
  - old cargo/cache files that are not currently open by a live process
  - old runner diagnostics/logs not needed by active jobs
- Preserve current/live files by default:
  - skip files newer than a configurable age threshold, default 7 days
  - skip files currently open according to `lsof`/`fuser` when available
  - skip active runner workspace paths for jobs currently in progress
  - never compress `.runner`, runner credentials/config, service units, git objects, or files matching a denylist
- Compress in place with a deterministic suffix such as `.zst`, using atomic temp files and fsync/rename:
  - write `<file>.zst.tmp`
  - verify decompression checksum
  - rename to `<file>.zst`
  - remove original only after verification
- Preserve access with one of two modes:
  - default: sidecar manifest maps original path to compressed file, size, mtime, mode, sha256, and compression timestamp
  - optional transparent mode: replace original with a tiny restore stub or symlink only when tools tolerate it
- Provide restore commands:
  - `fxrun cache restore <path>` restores one file
  - `fxrun cache restore --all --root <cache-root>` restores a tree
  - restores preserve original mode, mtime, and checksum
- Keep automation safe and auditable:
  - dry-run mode prints planned savings
  - JSON report includes compressed count, skipped count, bytes before/after, and reasons
  - per-run manifest under `_work/cache-compression/manifests/`
  - lock file prevents concurrent compression on the same runner home
- Schedule after runner idle windows, not during job execution:
  - systemd timer or GitHub workflow dispatch
  - optional runner health gate checks no active `Runner.Worker` for the target slot

## Acceptance criteria
- Red test proves live runner files and `.runner` files are never compressed.
- Unit tests cover age threshold, denylist, manifest, atomic write, and restore path.
- Integration dry-run over sample `_work` fixtures reports expected savings without mutations.
- Compression is idempotent: re-running does not double-compress or corrupt manifests.
- Restore round-trip verifies byte-for-byte equality and mode/mtime preservation.
- Documentation explains default roots, denylist, restore commands, and emergency rollback.

## Risks / guardrails
- Do not compress files required by active GitHub runner processes.
- Do not make cache paths inaccessible to existing tools unless a restore command is explicit and tested.
- Do not delete original files until compressed output and manifest verification pass.
- Do not use gitignore as the primary control; this feature is about preserving state more efficiently, not hiding it.

## Implementation evidence
- Added top-level `fxrun cache` command surface with `audit`, `compress`, and `restore` subcommands.
- Added zstd-backed in-place compression with `.zst.tmp` atomic write, decompression checksum verification, manifest metadata, and original removal only after verification.
- Added manifest/direct restore with checksum verification, overwrite policy, mode/mtime restore, dry-run, and verify-only modes.
- Added denylist and liveness guardrails for `.runner`, credentials, service/config identity files, `.git`, `_work/archives`, active paths, young files, and open files when `lsof` is available.
- Added JSON/human output contracts and command help for all three commands.
- Added `scripts/cache-maintenance.sh`, `.github/workflows/cache-maintenance.yml`, and `systemd/user/flexnetos-cache-maintenance.{service,timer}` trigger examples.
- Added `docs/cache-compression.md` with usage, defaults, safety model, restore procedure, wrapper env vars, and release gate.
- Validation evidence: red tests were created first; then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace --all-features`, `bash -n scripts/cache-maintenance.sh`, cache command help checks, cache audit JSON smoke, and forge-loop strict audits all passed locally.
