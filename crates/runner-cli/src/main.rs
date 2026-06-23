//! `fxrun` — operator CLI for `flexnetos_runner` (ADR-0008 §2). Shows how a job kind
//! routes (kernel + placement) and reports runner seam wiring.

use clap::{Parser, Subcommand};
use runner_core::{
    agent::Agent,
    jobspec::{JobKind, JobSpec},
    router, safety,
};

#[derive(Parser)]
#[command(name = "fxrun", version, about = "flexnetos_runner operator CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show which kernel a job kind routes to (`ci|review|agent|cycle`) and its placement.
    Route {
        kind: String,
        #[arg(long, default_value = "FlexNetOS/x")]
        repo: String,
        /// Simulate a fork-triggered job (forces hosted-only placement).
        #[arg(long)]
        fork: bool,
        /// Agent backend for agent-class jobs (`review|agent`): claude (default) | codex | kimi.
        #[arg(long, default_value = "claude", env = "FXRUN_AGENT")]
        agent: Agent,
    },
    /// List the supported agent backends and the current default, with the headless invocation.
    Agents,
    /// Report rails + seam wiring status.
    Doctor,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Route {
            kind,
            repo,
            fork,
            agent,
        } => {
            let job = sample(&kind, &repo, fork, agent)?;
            let plan = router::route(&job);
            let place = safety::placement(&job);
            let agent_note = match plan.agent {
                Some(a) => format!(" agent={a}"),
                None => String::new(),
            };
            println!(
                "kernel={:?} ({}) placement={:?}{} intent='{}'",
                plan.kernel,
                plan.kernel.program(),
                place,
                agent_note,
                plan.intent
            );
        }
        Cmd::Agents => {
            println!("fxrun agent backends (default first):");
            for a in Agent::ALL {
                let inv = a.invocation();
                let default_mark = if a.is_default() { "  [default]" } else { "" };
                let model = inv.model.unwrap_or("(backend/env default)");
                println!("  {:<7}{}", a.as_str(), default_mark);
                println!("    api    : {:?}", a.api_style());
                println!(
                    "    spawn  : {} {} {} {}",
                    inv.program,
                    inv.subcommand.join(" "),
                    inv.headless_flags.join(" "),
                    inv.structured_output.join(" ")
                );
                println!("    model  : {model}");
                if !inv.env.is_empty() {
                    let env = inv
                        .env
                        .iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    println!("    env    : {env}");
                }
            }
        }
        Cmd::Doctor => {
            let rails = safety::Rails::default();
            println!("fxrun");
            println!("  rails safe         : {}", rails.is_safe());
            println!("  labels             : {:?}", rails.labels);
            println!(
                "  agent backends     : {} (default: {})",
                Agent::ALL
                    .iter()
                    .map(|a| a.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                Agent::default()
            );
            let breaker = runner_core::LoopGuard::default();
            println!(
                "  loop breaker       : {} identical / window {} (FXRUN_LOOP_THRESHOLD/_WINDOW)",
                breaker.trip_threshold(),
                breaker.window()
            );
            println!(
                "  dispatch budget    : unlimited by default — jobs/tokens/USD caps via \
                 FXRUN_DISPATCH_BUDGET / FXRUN_TOKEN_BUDGET / FXRUN_USD_MICROS_BUDGET"
            );
            println!(
                "  survival tiers     : full → conserving (75%) → distress (90%) → halted; \
                 debounced floor FXRUN_BUDGET_GRACE admits past a met cap (0 = strict cliff)"
            );
            println!(
                "  cost seam          : atc→runner per-job JobCost (tokens+USD); charged to the budget"
            );
            let recovery = runner_core::RecoveryPolicy::default();
            println!(
                "  approval gate      : off by default (FXRUN_APPROVAL_BANDS=ci,review,agent,cycle \
                 → hold flagged classes until a human grant)"
            );
            println!(
                "  structural lint    : malformed jobs (bad repo / blank head_sha / pr_number 0) \
                 refused before the kernel"
            );
            println!(
                "  recovery routing   : {} retries / {}s base backoff, then escalate-to-human \
                 (FXRUN_MAX_RETRIES/_RETRY_BACKOFF_SECS)",
                recovery.max_retries(),
                recovery.base_backoff_secs()
            );
            println!(
                "  quarantine gate    : off by default (FXRUN_QUARANTINE_THRESHOLD=N → after N \
                 kernel failures of a fingerprint, refuse re-dispatch until re-armed)"
            );
            println!(
                "  dispatch deadline  : off by default (FXRUN_DEFAULT_DEADLINE_SECS caps wall-clock \
                 per job; a hung delegation times out → recovery + quarantine; tighter per-job on the \
                 request envelope)"
            );
            println!(
                "  rate limit         : off by default (FXRUN_RATE_MAX per FXRUN_RATE_WINDOW_SECS \
                 rolling window + FXRUN_ROUTE_COOLDOWN_SECS per-route failure backoff; refusals carry \
                 a retry-after, never escalate)"
            );
            println!(
                "  content scan       : off by default (FXRUN_SCAN_BLOCK_SEVERITY=low|medium|high|\
                 critical → refuse a job whose free-text fields trip the injection pattern bank at/above \
                 the threshold; escalates, never retries)"
            );
            println!(
                "  audit log          : off by default (FXRUN_EVENT_LOG → NDJSON dispatch trail)"
            );
            println!(
                "  secret redaction   : dispatch key + FXRUN_REDACT_SECRETS scrubbed from the audit \
                 log + error replies (active whenever serving; Archon repo.ts token scrub)"
            );
            println!(
                "  policy stream      : off by default (FXRUN_POLICY_LOG → admission/guardrail \
                 decisions only, for tamper-lineage)"
            );
            println!(
                "  constitution       : off by default (FXRUN_CONSTITUTION → seal governing files; \
                 mid-run change halts dispatch)"
            );
            println!(
                "  workspace teardown : guaranteed on every exit path incl. failure (Archon \
                 zero-residue); tmpfs worktree in P3"
            );
            println!("  actions supervisor : WIRED (fxrun-actions install/register/run-once)");
            println!("  uds dispatch       : UNWIRED (P2)");
            println!("  secret injection   : UNWIRED (P3 — envctl relay-bearer)");
        }
    }
    Ok(())
}

fn sample(kind: &str, repo: &str, fork: bool, agent: Agent) -> anyhow::Result<JobSpec> {
    let job = match kind {
        "ci" => JobKind::Ci {
            repo: repo.into(),
            head_sha: "HEAD".into(),
        },
        "review" => JobKind::ReviewGate {
            repo: repo.into(),
            pr_number: 1,
            head_sha: "HEAD".into(),
            agent,
        },
        "agent" => JobKind::AgentTask {
            repo: repo.into(),
            prompt_ref: "demo".into(),
            agent,
        },
        "cycle" => JobKind::LoopCycle {
            repo: repo.into(),
            task_id: "T-1".into(),
        },
        other => anyhow::bail!("unknown kind '{other}' (expected ci|review|agent|cycle)"),
    };
    Ok(JobSpec {
        id: "sample".into(),
        correlation_id: "sample".into(),
        from_fork: fork,
        job,
    })
}
