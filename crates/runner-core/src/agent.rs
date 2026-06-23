//! Agent backend selection (ADR-0008 §2): *which* coding agent executes an agent-class job.
//!
//! The runner is **delegate-only** — it never drives an LLM itself. For agent-class jobs
//! ([`crate::jobspec::JobKind::AgentTask`] / [`crate::jobspec::JobKind::ReviewGate`]) it routes
//! to the `atc` kernel ([`crate::router`]) and tells `atc` *which* agent backend to spawn. This
//! module is that selector: a small, serde-stable enum the App can put on a [`crate::jobspec`]
//! and the dispatcher carries into the [`crate::router::KernelPlan`].
//!
//! **Claude is the default ("Claude right now").** A job that names no agent is Claude — both at
//! the type level ([`Default`]) and on the wire (`#[serde(default)]` on the job fields), so an
//! older App that emits no `agent` field still dispatches to Claude. Adding the field is
//! therefore backward-compatible: the App signs bytes, not a shared struct (see
//! `flexnetos_github_app::dispatch`), and an absent field decodes to the default.
//!
//! Picking the *agent* (this module) is orthogonal to picking the *kernel* ([`crate::router`]):
//! the kernel for an agent-class job is always `atc`; the agent is the model-CLI backend `atc`
//! invokes. Keeping it here, not as a new `Kernel`, preserves delegate-only.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// A coding-agent backend the runner can ask `atc` to drive for an agent-class job.
///
/// Extend by adding a variant here and to [`Agent::ALL`]; everything else (serde, CLI parsing,
/// doctor output) is derived from this one enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agent {
    /// Anthropic Claude Code — the default backend ("Claude right now").
    #[default]
    Claude,
    /// OpenAI Codex CLI.
    Codex,
    /// Moonshot Kimi (Anthropic-compatible API surface).
    Kimi,
}

impl Agent {
    /// Every supported backend, in priority order (default first). The single source of truth
    /// for CLI help, doctor listings, and exhaustiveness in tests.
    pub const ALL: [Agent; 3] = [Agent::Claude, Agent::Codex, Agent::Kimi];

    /// The canonical, wire-stable identifier (matches the serde representation).
    pub fn as_str(&self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Kimi => "kimi",
        }
    }

    /// The CLI binary `atc` spawns for this backend (the runner names it; `atc` invokes it).
    ///
    /// Kimi exposes an Anthropic-compatible API, so it is driven through the same `claude` CLI
    /// pointed at Moonshot via [`Agent::base_url_env`]; Codex ships its own `codex` binary.
    pub fn program(&self) -> &'static str {
        match self {
            Agent::Claude | Agent::Kimi => "claude",
            Agent::Codex => "codex",
        }
    }

    /// The wire protocol `atc` talks to drive this backend. Distinguishes "same CLI, different
    /// endpoint" (Claude vs Kimi) from a genuinely different tool (Codex).
    pub fn api_style(&self) -> ApiStyle {
        match self {
            Agent::Claude => ApiStyle::AnthropicNative,
            Agent::Kimi => ApiStyle::AnthropicCompatible,
            Agent::Codex => ApiStyle::OpenAiCodex,
        }
    }

    /// For an Anthropic-compatible backend, the env var `atc` sets to retarget the `claude` CLI
    /// at the provider's endpoint (e.g. Kimi → `ANTHROPIC_BASE_URL`). `None` for native backends
    /// that need no base-URL override.
    pub fn base_url_env(&self) -> Option<&'static str> {
        match self.api_style() {
            ApiStyle::AnthropicCompatible => Some("ANTHROPIC_BASE_URL"),
            ApiStyle::AnthropicNative | ApiStyle::OpenAiCodex => None,
        }
    }

    /// Whether this is the default backend ("Claude right now").
    pub fn is_default(&self) -> bool {
        *self == Agent::default()
    }

    /// The canonical **headless, deterministic** invocation spec `atc` uses to drive this backend
    /// (current as of June 2026). This is *data the runner hands `atc`* — the runner still never
    /// spawns the model itself, so delegate-only (ADR-0008 §2) holds. `atc` composes
    /// `program + subcommand + headless_flags + structured_output (+ --model model) + the prompt`,
    /// with `env` exported first.
    ///
    /// Sources: Claude Code headless docs (`--bare`, `--permission-mode dontAsk`,
    /// `--output-format json`); OpenAI `codex exec` (`--sandbox workspace-write`,
    /// `--ask-for-approval never`, `--ignore-user-config`, `--json`); Moonshot Kimi agent-support
    /// (Anthropic-compatible base-URL swap on the same `claude` CLI).
    pub fn invocation(&self) -> AgentInvocation {
        match self {
            // `claude --bare -p <prompt> --permission-mode dontAsk --output-format json --model claude-opus-4-8`
            Agent::Claude => AgentInvocation {
                program: "claude",
                subcommand: &["-p"],
                headless_flags: &["--bare", "--permission-mode", "dontAsk"],
                structured_output: &["--output-format", "json"],
                model: Some("claude-opus-4-8"),
                env: &[],
            },
            // `codex exec <task> --sandbox workspace-write --ask-for-approval never --ignore-user-config --json`
            Agent::Codex => AgentInvocation {
                program: "codex",
                subcommand: &["exec"],
                headless_flags: &[
                    "--sandbox",
                    "workspace-write",
                    "--ask-for-approval",
                    "never",
                    "--ignore-user-config",
                ],
                structured_output: &["--json"],
                model: None, // Codex selects its own default; pin via codex-args if needed.
                env: &[],
            },
            // Same `claude` CLI, retargeted at Moonshot's Anthropic-compatible endpoint via env.
            Agent::Kimi => AgentInvocation {
                program: "claude",
                subcommand: &["-p"],
                headless_flags: &["--bare", "--permission-mode", "dontAsk"],
                structured_output: &["--output-format", "json"],
                model: None, // model is selected through ANTHROPIC_MODEL below, not `--model`.
                env: &[
                    ("ANTHROPIC_BASE_URL", "https://api.moonshot.ai/anthropic"),
                    ("ANTHROPIC_MODEL", "kimi-k2.7-code"),
                ],
            },
        }
    }
}

/// The concrete headless invocation `atc` assembles for a backend. Static slices (no allocation);
/// the prompt/task is appended by `atc` at spawn time. Keeping it as declarative data (not a
/// `Command`) is what lets the runner stay delegate-only while still pinning the *current* best
/// invocation in one auditable place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentInvocation {
    /// CLI binary `atc` spawns (`claude` or `codex`).
    pub program: &'static str,
    /// Non-interactive subcommand (`-p` print mode / `exec`).
    pub subcommand: &'static [&'static str],
    /// Determinism + no-prompt flags for reproducible unattended runs.
    pub headless_flags: &'static [&'static str],
    /// Flags that make stdout machine-readable (JSON), so the result is witnessable.
    pub structured_output: &'static [&'static str],
    /// Default model id passed via `--model`, when the runner pins one (`None` → backend default
    /// or env-selected).
    pub model: Option<&'static str>,
    /// Env vars `atc` must export before spawning (e.g. Kimi's base-URL + model retarget).
    pub env: &'static [(&'static str, &'static str)],
}

/// How `atc` drives a backend: same `claude` CLI on the native endpoint, the same CLI retargeted
/// at an Anthropic-compatible endpoint, or the separate `codex` CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiStyle {
    AnthropicNative,
    AnthropicCompatible,
    OpenAiCodex,
}

/// Parse an [`Agent`] from a CLI value / env string. Case-insensitive; accepts a few common
/// aliases so operators aren't surprised. Unknown values list the supported set.
impl FromStr for Agent {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-code" | "anthropic" | "" => Ok(Agent::Claude),
            "codex" | "openai" | "openai-codex" => Ok(Agent::Codex),
            "kimi" | "kimi-k2" | "moonshot" => Ok(Agent::Kimi),
            other => Err(format!(
                "unknown agent '{other}' (expected one of: {})",
                Agent::ALL
                    .iter()
                    .map(|a| a.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}

impl std::fmt::Display for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_is_the_default() {
        assert_eq!(Agent::default(), Agent::Claude);
        assert!(Agent::Claude.is_default());
        assert!(!Agent::Codex.is_default());
        assert!(!Agent::Kimi.is_default());
    }

    #[test]
    fn as_str_matches_serde_repr() {
        for a in Agent::ALL {
            let json = serde_json::to_string(&a).unwrap();
            assert_eq!(json, format!("\"{}\"", a.as_str()));
        }
    }

    #[test]
    fn from_str_roundtrips_and_accepts_aliases() {
        for a in Agent::ALL {
            assert_eq!(a.as_str().parse::<Agent>().unwrap(), a);
        }
        assert_eq!("CLAUDE".parse::<Agent>().unwrap(), Agent::Claude);
        assert_eq!("claude-code".parse::<Agent>().unwrap(), Agent::Claude);
        assert_eq!("openai".parse::<Agent>().unwrap(), Agent::Codex);
        assert_eq!("moonshot".parse::<Agent>().unwrap(), Agent::Kimi);
        assert_eq!("".parse::<Agent>().unwrap(), Agent::Claude); // empty → default
    }

    #[test]
    fn from_str_rejects_unknown_and_lists_options() {
        let err = "gpt5".parse::<Agent>().unwrap_err();
        assert!(err.contains("claude"));
        assert!(err.contains("codex"));
        assert!(err.contains("kimi"));
    }

    #[test]
    fn programs_and_api_styles_are_consistent() {
        // Claude and Kimi share the Anthropic CLI; Kimi differs only by base-URL retarget.
        assert_eq!(Agent::Claude.program(), "claude");
        assert_eq!(Agent::Kimi.program(), "claude");
        assert_eq!(Agent::Codex.program(), "codex");

        assert_eq!(Agent::Claude.api_style(), ApiStyle::AnthropicNative);
        assert_eq!(Agent::Kimi.api_style(), ApiStyle::AnthropicCompatible);
        assert_eq!(Agent::Codex.api_style(), ApiStyle::OpenAiCodex);

        // Only the compatible backend needs a base-URL override.
        assert_eq!(Agent::Claude.base_url_env(), None);
        assert_eq!(Agent::Kimi.base_url_env(), Some("ANTHROPIC_BASE_URL"));
        assert_eq!(Agent::Codex.base_url_env(), None);
    }

    #[test]
    fn invocations_are_headless_deterministic_and_structured() {
        for a in Agent::ALL {
            let inv = a.invocation();
            assert!(
                !inv.subcommand.is_empty(),
                "{a} needs a headless subcommand"
            );
            assert!(
                !inv.structured_output.is_empty(),
                "{a} must request structured output so the result is witnessable"
            );
            // program() and the invocation program agree.
            assert_eq!(inv.program, a.program());
        }
    }

    #[test]
    fn claude_default_pins_the_current_flagship_model() {
        let inv = Agent::Claude.invocation();
        assert_eq!(inv.program, "claude");
        assert_eq!(inv.subcommand, &["-p"]);
        assert!(inv.headless_flags.contains(&"--bare"));
        assert!(inv.headless_flags.contains(&"dontAsk"));
        assert_eq!(inv.model, Some("claude-opus-4-8"));
        assert!(inv.env.is_empty());
    }

    #[test]
    fn codex_uses_exec_with_no_approval_prompt() {
        let inv = Agent::Codex.invocation();
        assert_eq!(inv.program, "codex");
        assert_eq!(inv.subcommand, &["exec"]);
        assert!(inv.headless_flags.contains(&"workspace-write"));
        assert!(inv.headless_flags.contains(&"never")); // --ask-for-approval never
        assert!(inv.structured_output.contains(&"--json"));
    }

    #[test]
    fn kimi_reuses_the_claude_cli_retargeted_via_env() {
        let inv = Agent::Kimi.invocation();
        assert_eq!(
            inv.program, "claude",
            "Kimi rides the Anthropic-compatible CLI"
        );
        // The base-URL env named by base_url_env() actually appears in the invocation env.
        let key = Agent::Kimi.base_url_env().unwrap();
        let base = inv.env.iter().find(|(k, _)| *k == key);
        assert_eq!(
            base.map(|(_, v)| *v),
            Some("https://api.moonshot.ai/anthropic")
        );
        assert!(inv.env.iter().any(|(k, _)| *k == "ANTHROPIC_MODEL"));
    }

    #[test]
    fn agent_field_defaults_to_claude_when_absent_on_the_wire() {
        // The backward-compat guarantee: a job emitted without an `agent` decodes to Claude.
        #[derive(Deserialize)]
        struct HasAgent {
            #[serde(default)]
            agent: Agent,
        }
        let decoded: HasAgent = serde_json::from_str("{}").unwrap();
        assert_eq!(decoded.agent, Agent::Claude);
    }
}
