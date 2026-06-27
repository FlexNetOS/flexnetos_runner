#!/usr/bin/env python3
"""Post-tool witness that reruns the lightweight component surface check."""
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
    ".codex/hooks.json",
    ".codex/hooks/forge_loop_pre_tool_use.py",
    ".codex/hooks/forge_loop_post_tool_use.py",
    ".codex/hooks/forge_loop_permission_request.py",
    ".codex/hooks/forge_loop_compact_summary.py",
    "docs/forge-loop/codex-target-mining.md",
]
missing = [path for path in REQUIRED if not (ROOT / path).exists()]
print(json.dumps({"forge_loop_post_tool_use": {"missing": missing}}, sort_keys=True))
