//! `fxrun` — operator CLI for `flexnetos_runner` (ADR-0008 §2). Shows how a job kind
//! routes (kernel + placement) and reports runner seam wiring.

use clap::{Parser, Subcommand};
use runner_core::{
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
    },
    /// Report rails + seam wiring status.
    Doctor,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Route { kind, repo, fork } => {
            let job = sample(&kind, &repo, fork)?;
            let plan = router::route(&job);
            let place = safety::placement(&job);
            println!(
                "kernel={:?} ({}) placement={:?} intent='{}'",
                plan.kernel,
                plan.kernel.program(),
                place,
                plan.intent
            );
        }
        Cmd::Doctor => {
            let rails = safety::Rails::default();
            println!("fxrun");
            println!("  rails safe         : {}", rails.is_safe());
            println!("  labels             : {:?}", rails.labels);
            println!("  actions supervisor : WIRED (fxrun-actions install/register/run-once)");
            println!("  uds dispatch       : UNWIRED (P2)");
            println!("  secret injection   : UNWIRED (P3 — envctl relay-bearer)");
        }
    }
    Ok(())
}

fn sample(kind: &str, repo: &str, fork: bool) -> anyhow::Result<JobSpec> {
    let job = match kind {
        "ci" => JobKind::Ci {
            repo: repo.into(),
            head_sha: "HEAD".into(),
        },
        "review" => JobKind::ReviewGate {
            repo: repo.into(),
            pr_number: 1,
            head_sha: "HEAD".into(),
        },
        "agent" => JobKind::AgentTask {
            repo: repo.into(),
            prompt_ref: "demo".into(),
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
