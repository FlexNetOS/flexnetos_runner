# Architect plan

VERDICT: GO.

1. Make Kache the only cache owner and retire generic cache audit/compress/restore automation.
2. Add Nushell runner policy/install/service entrypoints; retained `.sh` scripts are owner-only break-glass and unreachable automatically.
3. Run official runner ELF executables directly, with JIT/ephemeral one-job processes and volatile work under `/run/user/1001/yazelix/runners`.
4. Set the runner shell to the profile Nushell and the Rust wrapper to the profile Kache wrapper.
5. Remove remote cache directives and automatic Bash/sh/zsh from workflows before runner re-enable.
