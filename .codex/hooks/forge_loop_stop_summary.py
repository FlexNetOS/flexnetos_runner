#!/usr/bin/env python3
"""Stop-hook component summary for the forge-loop harness."""
from __future__ import annotations

import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
components = {
    "project_config": ".codex/config.toml",
    "compact_prompt": ".codex/prompts/compact-forge-loop.md",
    "hooks": ".codex/hooks.json",
    "rules": ".codex/rules/forge-loop.rules",
    "subagent": ".codex/agents/forge-loop-auditor.toml",
    "skill": ".agents/skills/forge-loop-research/SKILL.md",
}
status = {name: (ROOT / path).exists() for name, path in components.items()}
print(json.dumps({"forge_loop_stop_summary": status}, sort_keys=True))
