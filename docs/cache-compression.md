# Cache compression automation

`fxrun cache` preserves runner cache evidence while making cold cache state cheaper to keep.
It is intentionally not a gitignore feature: the cache topology remains visible, audit reports are
machine-readable, and compressed files can be restored byte-for-byte.

## Commands

### `fxrun cache audit`

Read-only inventory. Defaults to `_work` and a 7 day cold-file threshold.

```bash
fxrun cache audit --root _work --slot all --min-age 7d --format json
```

The JSON report lists selected candidates and skipped files with reasons. Denied paths include
`.runner`, credential-like files, service/config identity files, `.git`, and `_work/archives` unless
a future explicit archive mode is added.

### `fxrun cache compress`

Compresses only files selected by the audit and re-checks safety before each file. It writes
`<path>.zst.tmp`, verifies decompression sha256, renames to `<path>.zst`, writes a manifest, and
removes the original only after verification.

```bash
fxrun cache compress --root _work --slot 01 --min-age 7d --dry-run --format json
fxrun cache compress --root _work --slot 01 --min-age 7d --format json
```

Manifests default to `_work/cache-compression/manifests/cache-compression-<timestamp>.json` and
contain original path, compressed path, size, mtime, mode, sha256, and compressor.

### `fxrun cache restore`

Restores from a manifest or verifies without writing.

```bash
fxrun cache restore --root _work --manifest _work/cache-compression/manifests/example.json --all --verify-only --format json
fxrun cache restore --root _work --manifest _work/cache-compression/manifests/example.json --all --format json
```

The default overwrite policy is `if-missing`; use `--overwrite never` for emergency inspection and
`--overwrite always` only for an intentional rollback.

## Automation triggers

- Manual workflow: `.github/workflows/cache-maintenance.yml`
- Local wrapper: `scripts/cache-maintenance.sh`
- Recommended scheduled trigger: systemd timer templates in `systemd/user/flexnetos-cache-maintenance.{service,timer}`. Replace `@FXRUN_PREFIX@` with the released install prefix. The checked-in template runs `scripts/cache-maintenance.sh` with `FXRUN_CACHE_MAINTENANCE_MODE=audit` and `FXRUN_CACHE_DRY_RUN=1` by default; enable compression only during a proven idle window.
- Pre-compression connector: `fxrun cache compress` internally calls the audit path.

## Guardrails

- Default `--dry-run` in the wrapper.
- Per-slot lock prevents concurrent compression on the same root/slot.
- Young files are skipped by `--min-age`.
- Open files are skipped when `lsof` is available.
- Tests cover denied/live paths, dry-run mutation safety, idempotency, restore round-trip, and
overwrite-policy blocking.
- Archives under `_work/archives` are out of scope by default.

## Environment overrides

Only `FXRUN_CACHE_*` environment variables are used by the wrapper and runtime active-path seam:

- `FXRUN_CACHE_ROOT`
- `FXRUN_CACHE_SLOT`
- `FXRUN_CACHE_MIN_AGE`
- `FXRUN_CACHE_FORMAT`
- `FXRUN_CACHE_MAINTENANCE_MODE`
- `FXRUN_CACHE_DRY_RUN`
- `FXRUN_CACHE_RESTORE_MANIFEST`
- `FXRUN_CACHE_ACTIVE_PATHS`

## Release gate

Before publishing a cache-compression change, run:

```bash
cargo fmt --all -- --check
cargo test -p runner-cli cache_
bash -n scripts/cache-maintenance.sh
cargo run -p runner-cli -- cache audit --root _work --slot all --min-age 3650d --format json
```
