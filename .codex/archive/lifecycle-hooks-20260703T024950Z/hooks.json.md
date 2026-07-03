{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup|resume",
        "hooks": [
          {
            "type": "command",
            "command": "/usr/bin/python3 \"$(git rev-parse --show-toplevel)/.codex/hooks/forge_loop_session_start.py\"",
            "timeout": 30,
            "statusMessage": "Checking forge-loop session readiness"
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/usr/bin/python3 \"$(git rev-parse --show-toplevel)/.codex/hooks/forge_loop_stop_summary.py\"",
            "timeout": 30,
            "statusMessage": "Summarizing forge-loop component status"
          }
        ]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "Bash|apply_patch|Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "/usr/bin/python3 \"$(git rev-parse --show-toplevel)/.codex/hooks/forge_loop_pre_tool_use.py\"",
            "timeout": 30,
            "statusMessage": "Witnessing forge-loop component surfaces"
          }
        ]
      }
    ],
    "SubagentStart": [
      {
        "matcher": ".*",
        "hooks": [
          {
            "type": "command",
            "command": "/usr/bin/python3 \"$(git rev-parse --show-toplevel)/.codex/hooks/forge_loop_subagent_summary.py\"",
            "timeout": 30,
            "statusMessage": "Recording forge-loop subagent roster"
          }
        ]
      }
    ],
    "SubagentStop": [
      {
        "matcher": ".*",
        "hooks": [
          {
            "type": "command",
            "command": "/usr/bin/python3 \"$(git rev-parse --show-toplevel)/.codex/hooks/forge_loop_subagent_summary.py\"",
            "timeout": 30,
            "statusMessage": "Summarizing forge-loop subagent roster"
          }
        ]
      }
    ],
    "PermissionRequest": [
      {
        "matcher": "Bash|apply_patch|Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "/usr/bin/python3 \"$(git rev-parse --show-toplevel)/.codex/hooks/forge_loop_permission_request.py\"",
            "timeout": 30,
            "statusMessage": "Checking forge-loop permission posture"
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Bash|apply_patch|Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "/usr/bin/python3 \"$(git rev-parse --show-toplevel)/.codex/hooks/forge_loop_post_tool_use.py\"",
            "timeout": 30,
            "statusMessage": "Rechecking forge-loop surfaces"
          }
        ]
      }
    ],
    "PreCompact": [
      {
        "matcher": "manual|auto",
        "hooks": [
          {
            "type": "command",
            "command": "/usr/bin/python3 \"$(git rev-parse --show-toplevel)/.codex/hooks/forge_loop_compact_summary.py\"",
            "timeout": 30,
            "statusMessage": "Summarizing target mining before compact"
          }
        ]
      }
    ],
    "PostCompact": [
      {
        "matcher": "manual|auto",
        "hooks": [
          {
            "type": "command",
            "command": "/usr/bin/python3 \"$(git rev-parse --show-toplevel)/.codex/hooks/forge_loop_compact_summary.py\"",
            "timeout": 30,
            "statusMessage": "Restoring target mining after compact"
          }
        ]
      }
    ]
  }
}
