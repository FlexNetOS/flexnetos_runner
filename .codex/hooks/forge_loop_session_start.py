#!/usr/bin/env python3
"""Session-start readiness hint for the forge-loop harness."""
from __future__ import annotations

import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
required = [
    ".codex/config.toml",
    ".codex/hooks.json",
    ".codex/rules/forge-loop.rules",
    ".codex/agents/forge-loop-auditor.toml",
    ".agents/skills/forge-loop-research/SKILL.md",
]
missing = [path for path in required if not (ROOT / path).exists()]
print(json.dumps({"forge_loop_session_start": {"missing": missing}}, sort_keys=True))
