#!/usr/bin/env python3
"""Permission-request witness for forge-loop sessions."""
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


root = repo_root()
config = tomllib.loads((root / ".codex" / "config.toml").read_text())
blueprint = tomllib.loads((root / ".codex" / "permissions" / "forge-loop-workspace.toml").read_text())
print(json.dumps({
    "forge_loop_permission_request": {
        "active_sandbox_mode": config.get("sandbox_mode"),
        "blueprint_default_permissions": blueprint.get("default_permissions"),
        "profile_is_blueprint_only": "default_permissions" not in config,
    }
}, sort_keys=True))
