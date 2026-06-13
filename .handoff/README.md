# .handoff (Tier-A, git-text-only)

Per ADR-0004 §3: this per-repo `.handoff/` carries **git-committed text only** — no
`ledger.db`, no binary state. Witnessed events for this repo are checkpointed into the
FLEET ledger at `meta/.handoff/ledger.db`. This repo's `packets/` are compiled centrally
by `hf fleet status` (kernel verb, not yet built) — never rendered from a per-repo ledger.
