# Runner State Settle

`fxrun runner-state audit|normalize|settle` is the preserve-state hygiene gate for the repo-local runner tree. It keeps `_work` visible, separates safe producer churn from blocking identity files, normalizes only proven benign runtime drift, and leaves snapshot/commit decisions explicit.

## Commands

```bash
fxrun runner-state audit --format json
fxrun runner-state normalize --format json
fxrun runner-state settle --slots all --commit --push-pr
```

- `audit` is read-only. It classifies dirty/preserved paths as `live-runner-state`, `cache-state`, `config-churn`, `gitkb-runtime`, `denied-sensitive`, or `unclassified`.
- `normalize` atomically dedupes repeated `safe.directory` entries in `_work/runner-home-*/.gitconfig`, removes GitKB runtime caches/workspaces after GitKB has its durable store, and removes only broad `_work` gitignore rules.
- `settle` composes audit + normalize, optionally commits an intentional runner-state snapshot, and can push/create an auto-merge PR when requested. It never owns cache compression; Kache is the only cache owner.

## Guardrails

The command does **not** delete runner identity files. `.runner`, `.credentials`, `.credentials_rsaparams`, `.env`, `.service`, token-like paths, and unclassified paths fail closed under strict gates. `.runner_migrated` is preserved as live runner state, not treated as trash.

`fxrun runner-state settle --require-idle` refuses to mutate if `pgrep -af 'Runner.Worker|Runner.Listener'` finds active runner processes unless `--force` is also passed.

## Better rule

Preserved runner state must be one of:

1. clean,
2. actively owned by a running worker,
3. compressed with a manifest,
4. normalized by `fxrun runner-state settle`, or
5. committed as an intentional runner-state snapshot.

That is the replacement for broad `_work/` gitignore hiding. Granular ignores for heavyweight generated payloads are still allowed when they preserve topology and evidence.

## Emergency behavior

Use `audit --strict --format json` before mutating. If strict reports `denied-sensitive` or `unclassified`, stop and review; do not normalize those paths. If only `config-churn` and `gitkb-runtime` are reported, `normalize` is safe to run. Purge legacy non-Kache cache state; do not preserve, compress, or restore it.
