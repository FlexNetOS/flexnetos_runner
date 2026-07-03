#!/usr/bin/env python3
"""Pre/PostCompact summary for target-mining continuity."""
from __future__ import annotations

import json
import pathlib
import subprocess
import tomllib

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


root = repo_root()
ledger = root / "docs" / "forge-loop" / "codex-target-mining.md"
config_path = root / ".codex" / "config.toml"
text = ledger.read_text() if ledger.exists() else ""
config = tomllib.loads(config_path.read_text()) if config_path.exists() else {}
covered = [target for target in TARGETS if target in text]
print(json.dumps({
    "forge_loop_compact_summary": {
        "target_count": len(TARGETS),
        "covered_targets": covered,
        "missing_targets": [target for target in TARGETS if target not in covered],
        "auto_compaction_enabled": config.get("features", {}).get("auto_compaction") is True,
        "compact_prompt": config.get("experimental_compact_prompt_file"),
        "auto_compact_token_limit": config.get("model_auto_compact_token_limit"),
        "active_phase": "preserve current red/implement/gate/evaluate/research/upgrade phase",
        "source_coverage": "covered_targets plus missing_targets",
        "validation_state": "preserve completed and remaining validation commands",
        "next_action": "preserve exactly one next action",
    }
}, sort_keys=True))
