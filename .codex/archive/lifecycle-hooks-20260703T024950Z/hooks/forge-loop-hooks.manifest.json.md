{
  "schema_version": 1,
  "purpose": "Machine-readable hook inventory for the Codex forge-loop harness.",
  "hooks": [
    {"event": "SessionStart", "script": ".codex/hooks/forge_loop_session_start.py", "expected_json_key": "forge_loop_session_start"},
    {"event": "PreToolUse", "script": ".codex/hooks/forge_loop_pre_tool_use.py", "expected_json_key": "forge_loop_pre_tool_use"},
    {"event": "PermissionRequest", "script": ".codex/hooks/forge_loop_permission_request.py", "expected_json_key": "forge_loop_permission_request"},
    {"event": "PostToolUse", "script": ".codex/hooks/forge_loop_post_tool_use.py", "expected_json_key": "forge_loop_post_tool_use"},
    {"event": "PreCompact", "script": ".codex/hooks/forge_loop_compact_summary.py", "expected_json_key": "forge_loop_compact_summary"},
    {"event": "PostCompact", "script": ".codex/hooks/forge_loop_compact_summary.py", "expected_json_key": "forge_loop_compact_summary"},
    {"event": "SubagentStart", "script": ".codex/hooks/forge_loop_subagent_summary.py", "expected_json_key": "forge_loop_subagents"},
    {"event": "SubagentStop", "script": ".codex/hooks/forge_loop_subagent_summary.py", "expected_json_key": "forge_loop_subagents"},
    {"event": "Stop", "script": ".codex/hooks/forge_loop_stop_summary.py", "expected_json_key": "forge_loop_stop_summary"}
  ]
}
