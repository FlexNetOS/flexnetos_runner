# `_work` preservation policy

`_work/` is the repo-local operations root for the FlexNetOS self-hosted runner fleet. It is
important state and must not be blanket-ignored.

Track durable, low-volume operational state directly in Git, including:

- runner slot topology and registration metadata needed for recovery (`_work/repos/actions-runner-*` selected dotfiles)
- service-name markers and runner environment files
- eval, queue, repair, and forge-loop evidence artifacts
- archive README files and checksum manifests

Do not track large/generated cache blobs directly, including:

- compressed recovery archives (`*.tar.zst`, `*.tar.gz`, etc.)
- build output directories (`target/`)
- downloaded Actions runner internals (`externals/`, `_actions/`, `_tool/`, `_temp/`)
- Rust/cargo cache and toolchain payloads (`.cargo/registry/`, `.rustup/toolchains/`)
- generated HTTP/model/blob caches (`.cache/gh/`, `.cache/icm/models/`, `.cache/kache/store/`)
- nested Git checkout internals (`.git/` under `_work`)
- nested package/advisory/plugin marketplaces such as `.cargo/advisory-db/` and
  `.claude/plugins/marketplaces/*/`
- nested runner worktree checkout roots that would otherwise be committed as broken gitlinks without
  `.gitmodules`; preserve their pipeline mapping and recovery evidence instead

The full byte-for-byte recovery archives remain on local storage and are referenced by checksum under
`_work/archives/`. If an archive must move off-host, use an artifact/backup system, not normal Git
blobs.
