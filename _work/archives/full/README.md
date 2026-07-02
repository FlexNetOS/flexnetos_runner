# Full runner recovery archive

This directory preserves the full `flexnetos_runner` recovery archive that contained the restored
`_work/` tree. It exists because raw Git cannot faithfully track nested Git working copies under
GitHub Actions work folders without turning them into gitlinks/submodules.

- `flexnetos_runner-full-20260702T104900Z.tar.zst` — original full repo/runtime recovery archive.
- `flexnetos_runner-full-20260702T104900Z.tar.zst.sha256` — checksum for the archive.

The active, extracted runner state is kept under repo-local `_work/`; this archive is the durable
byte-for-byte recovery source.
