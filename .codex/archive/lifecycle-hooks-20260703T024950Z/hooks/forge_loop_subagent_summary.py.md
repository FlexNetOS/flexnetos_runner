#!/usr/bin/env python3
"""Emit the project-scoped forge-loop subagent roster for SubagentStart/Stop hooks."""
from __future__ import annotations

import json
import pathlib
import subprocess
import tomllib


def repo_root() -> pathlib.Path:
    try:
        out = subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip()
        return pathlib.Path(out)
    except Exception:
        return pathlib.Path.cwd()


agents_dir = repo_root() / ".codex" / "agents"
agents = []
for path in sorted(agents_dir.glob("*.toml")):
    try:
        data = tomllib.loads(path.read_text())
        agents.append({"file": path.name, "name": data.get("name", path.stem), "sandbox_mode": data.get("sandbox_mode")})
    except Exception as exc:
        agents.append({"file": path.name, "error": str(exc)})
print(json.dumps({"forge_loop_subagents": agents}, sort_keys=True))
