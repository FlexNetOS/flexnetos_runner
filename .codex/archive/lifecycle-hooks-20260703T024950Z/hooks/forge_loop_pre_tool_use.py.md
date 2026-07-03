#!/usr/bin/env python3
"""Lightweight pre-tool surface witness for the forge-loop harness."""
from __future__ import annotations

import json
import pathlib
import subprocess


def repo_root() -> pathlib.Path:
    try:
        out = subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip()
        return pathlib.Path(out)
    except Exception:
        return pathlib.Path.cwd()


ROOT = repo_root()
REQUIRED = [
    ".codex/config.toml",
    ".codex/hooks.json",
    ".codex/prompts/compact-forge-loop.md",
    ".codex/rules/forge-loop.rules",
    ".codex/agents/forge-loop-auditor.toml",
    ".codex/agents/forge-loop-researcher.toml",
    ".codex/agents/forge-loop-ci-sentinel.toml",
    ".codex/permissions/forge-loop-workspace.toml",
    ".github/workflows/codex-forge-loop.yml",
    ".github/codex/schemas/forge-loop-output.schema.json",
]
missing = [path for path in REQUIRED if not (ROOT / path).exists()]
print(json.dumps({"forge_loop_pre_tool_use": {"missing": missing}}, sort_keys=True))
