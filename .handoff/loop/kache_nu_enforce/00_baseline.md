# Kache-only / Nushell-only baseline

- Base: `0398173`.
- Worktree: `/tmp/kache_nu_enforce/flexnetos_runner`
- Both local runner services are stopped and point at deleted repo-local entrypoints.
- The profile does not currently contain `fxrun`, `fxrun-actions`, or `fxrun-dispatch`.
- Current service generation creates persistent repo-local Cargo, Rustup, Bun, work, action, tool, temp, and cache state.
- Existing generic cache compression/maintenance conflicts with Kache-only ownership.
