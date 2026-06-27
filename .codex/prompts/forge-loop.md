---
description: Run the Rust-backed Codex TDD forge-loop seed.
---

Use the repository's Rust forge-loop engine instead of improvising the loop in chat.

Command:

```bash
fxrun forge-loop run --goal "$ARGUMENTS"
```

Rules:
- Follow TDD: prove or create a red test before implementation.
- Run self-evaluation on every cycle.
- Use research findings to improve reliability, accuracy, and speed.
- Commit, push, open a PR, and auto-merge green PRs when repository settings allow.
- Strict upgrade only: no downgrade/removal unless a replacement is installed, configured, and parity-proven.
