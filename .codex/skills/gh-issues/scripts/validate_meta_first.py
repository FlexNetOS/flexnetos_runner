#!/usr/bin/env python3
"""Validate FlexNetOS meta-first invariants in the gh-issues skill."""
from pathlib import Path
import sys

skill = Path(__file__).resolve().parents[1] / "SKILL.md"
text = skill.read_text(encoding="utf-8")
requirements = {
    "Meta-first FlexNetOS policy": "policy section header",
    "META_ROOT": "meta workspace root discovery",
    "/home/drdave/Desktop/meta": "fallback meta root",
    ".worktrees": "isolated meta-owned worktrees",
    "rtk": "RTK command prefix",
    "Strict upgrade only": "strict upgrade invariant",
    "commit, push, and open a PR immediately": "owner publication rule",
    "no duplicate active sessions": "session/claim de-duplication",
    "meta-owned state": "claim/cursor placement",
}
missing = [label for needle, label in requirements.items() if needle not in text]
if missing:
    print("Missing gh-issues meta-first requirements:", file=sys.stderr)
    for item in missing:
        print(f"- {item}", file=sys.stderr)
    sys.exit(1)
print(f"OK: {skill} contains {len(requirements)} meta-first requirements")
