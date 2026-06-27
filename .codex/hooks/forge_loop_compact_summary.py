#!/usr/bin/env python3
"""Pre/PostCompact summary for target-mining continuity."""
from __future__ import annotations

import json
import pathlib
import subprocess

TARGETS = [
    "developers.openai.com/codex/github-action",
    "developers.openai.com/codex/permissions",
    "developers.openai.com/codex/subagents",
    "RoggeOhta/awesome-codex-cli",
    "Yeachan-Heo/oh-my-codex",
]


def repo_root() -> pathlib.Path:
    try:
        out = subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip()
        return pathlib.Path(out)
    except Exception:
        return pathlib.Path.cwd()


ledger = repo_root() / "docs" / "forge-loop" / "codex-target-mining.md"
text = ledger.read_text() if ledger.exists() else ""
covered = [target for target in TARGETS if target in text]
print(json.dumps({
    "forge_loop_compact_summary": {
        "target_count": len(TARGETS),
        "covered_targets": covered,
        "missing_targets": [target for target in TARGETS if target not in covered],
    }
}, sort_keys=True))
