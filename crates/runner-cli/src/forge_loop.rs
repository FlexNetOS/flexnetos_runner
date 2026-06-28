use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_CODEX: &str = "/home/drdave/Desktop/meta/.toolchains/codex/bin/codex";
const DEFAULT_ARTIFACT_ROOT: &str = "_work/forge-loop";
const MAX_EVAL_RETRY_COUNT: u8 = 10;
const REQUIRED_LOCAL_CHECKS: &[&str] = &["Local Linux CI", "Semantic PR Title"];
const CYCLE_MANIFEST_SCHEMA_VERSION: u8 = 1;
const AUTO_COMPACT_TOKEN_LIMIT: u32 = 3_000_000;
const TOOL_OUTPUT_TOKEN_LIMIT: u32 = 12_000;
const COMPACT_PROMPT_PATH: &str = ".codex/prompts/compact-forge-loop.md";
const REQUIRED_GATE_COMMANDS: &[&str] = &[
    "cargo fmt --all -- --check",
    "cargo test -p runner-cli --all-features forge_loop::tests",
    "cargo run -q -p runner-cli -- forge-loop docs-drift --json",
    "cargo run -q -p runner-cli -- forge-loop target-mining-audit --json",
    "cargo run -q -p runner-cli -- forge-loop runner-flow-audit --json",
    "cargo run -q -p runner-cli -- forge-loop agentic-system-audit --json",
    "cargo test --workspace --all-features",
    "cargo clippy --workspace --all-targets --all-features -- -D warnings",
    "cargo audit --deny warnings",
];

#[derive(Subcommand, Debug, Clone)]
pub enum ForgeLoopCommand {
    /// Run one TDD forge-loop cycle: plan red test, implement with Codex, gate, evaluate, and propose an upgrade.
    Run(RunArgs),
    /// Evaluate a forge-loop run or fixture and emit the score used for self-upgrade decisions.
    Eval(EvalArgs),
    /// Scan configured research sources for loop reliability, accuracy, and speed improvements.
    Research(ResearchArgs),
    /// Turn the latest evaluation/research result into a self-upgrade PR plan or PR-producing Codex task.
    SelfUpgrade(SelfUpgradeArgs),
    /// Show local readiness for the Codex-backed forge loop.
    Doctor(DoctorArgs),
    /// Diagnose pending required checks that need local self-hosted runners.
    RunnerHealth(RunnerHealthArgs),
    /// Audit runner utilization and PR-flow evidence against the kclaw0 dark-factory target.
    RunnerFlowAudit(RunnerFlowAuditArgs),
    /// Audit observed runner history against the kclaw0 black-factor/dark-factory window target.
    RunnerBlackFactorAudit(RunnerBlackFactorAuditArgs),
    /// Audit unattended dark-factory operational SLO evidence over a burn-in window.
    RunnerOpsSloAudit(RunnerOpsSloAuditArgs),
    /// Audit live local self-hosted runner lane ownership, including cross-repo pressure.
    RunnerFleetAudit(RunnerFleetAuditArgs),
    /// Audit the full 24/7 agentic loop: research, evaluation, adaptation, growth, runners, and PR flow.
    AgenticSystemAudit(AgenticSystemAuditArgs),
    /// Fail when exported forge-loop upgrades are still documented as queued/backlog work.
    DocsDrift(DocsDriftArgs),
    /// Inventory Codex loop components and config surfaces for upgrade planning.
    ComponentsAudit(ComponentsAuditArgs),
    /// Verify required Codex target mining sources were extracted, applied, and guarded.
    TargetMiningAudit(TargetMiningAuditArgs),
}

#[derive(Args, Debug, Clone)]
pub struct RunArgs {
    /// Goal or backlog item for this cycle.
    #[arg(
        long,
        default_value = "advance the next highest-confidence forge-loop improvement"
    )]
    pub goal: String,
    /// Artifact root for cycle evidence.
    #[arg(long, default_value = DEFAULT_ARTIFACT_ROOT)]
    pub out: PathBuf,
    /// Print/write the planned cycle without invoking Codex or publishing changes.
    #[arg(long)]
    pub dry_run: bool,
    /// Attempt PR auto-merge after green checks for self-upgrade PRs.
    #[arg(long, default_value_t = true)]
    pub auto_merge: bool,
    /// Stop after one cycle. The seed intentionally defaults to one cycle until supervised by a scheduler.
    #[arg(long, default_value_t = true)]
    pub once: bool,
}

#[derive(Args, Debug, Clone)]
pub struct EvalArgs {
    /// Emit a deterministic fixture score for smoke tests and CI.
    #[arg(long)]
    pub fixture: bool,
    /// Optional metrics JSON file from a prior run.
    #[arg(long)]
    pub metrics: Option<PathBuf>,
    /// Optional cycle manifest whose prompt/phase contract must match the run before scoring.
    #[arg(long)]
    pub manifest: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct ResearchArgs {
    /// Print the research plan without invoking Codex.
    #[arg(long)]
    pub dry_run: bool,
    /// Focus area for research agents.
    #[arg(long, default_value = "reliability, accuracy, and speed")]
    pub focus: String,
}

#[derive(Args, Debug, Clone)]
pub struct SelfUpgradeArgs {
    /// Print the upgrade plan without invoking Codex or gh.
    #[arg(long)]
    pub dry_run: bool,
    /// Minimum evaluation score required to request an autonomous self-upgrade.
    #[arg(long, default_value_t = 70)]
    pub min_score: u8,
}

#[derive(Args, Debug, Clone)]
pub struct DoctorArgs {
    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct RunnerHealthArgs {
    /// JSON from `gh pr view <PR> --json statusCheckRollup`.
    #[arg(long)]
    pub checks_json: PathBuf,
    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct RunnerBlackFactorAuditArgs {
    /// JSON from `gh run list --json name,status,conclusion,createdAt,updatedAt,event,url`.
    #[arg(long)]
    pub runs_json: PathBuf,
    /// JSON from `gh pr list --state all --json state,mergedAt,statusCheckRollup,url`.
    #[arg(long)]
    pub prs_json: PathBuf,
    /// Minimum observed wall-clock window in hours.
    #[arg(long, default_value_t = 12)]
    pub min_window_hours: u64,
    /// Minimum duration-proven successful Runner Sustain workflow runs in the window.
    #[arg(long, default_value_t = 72)]
    pub min_sustain_runs: usize,
    /// Minimum wall-clock duration required before a Runner Sustain success counts as useful work.
    #[arg(long, default_value_t = 5)]
    pub min_sustain_duration_minutes: u64,
    /// Minimum merged PRs with clean required checks in the window.
    #[arg(long, default_value_t = 1)]
    pub min_clean_merged_prs: usize,
    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,
    /// Return non-zero when the observed window does not exceed the target.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Args, Debug, Clone)]
pub struct RunnerOpsSloAuditArgs {
    /// JSON from `gh run list --json name,status,conclusion,createdAt,updatedAt,event,url`.
    #[arg(long)]
    pub runs_json: PathBuf,
    /// JSON from `gh pr list --state open --json statusCheckRollup,url`.
    #[arg(long)]
    pub prs_json: PathBuf,
    /// Minimum observed burn-in window in hours.
    #[arg(long, default_value_t = 1)]
    pub min_window_hours: u64,
    /// Maximum allowed observed idle gap between useful Runner Sustain intervals.
    #[arg(long, default_value_t = 10)]
    pub max_idle_gap_minutes: u64,
    /// Minimum active/queued Runner Sustain backlog required at audit time.
    #[arg(long, default_value_t = 1)]
    pub min_active_or_queued_sustain: usize,
    /// Minimum successful event-driven Runner Black Factor Watch runs in the window.
    #[arg(long, default_value_t = 1)]
    pub min_event_watch_wakeups: usize,
    /// Maximum failed operational workflow runs allowed in the window.
    #[arg(long, default_value_t = 0)]
    pub max_failed_ops_runs: usize,
    /// Minimum wall-clock duration required before a completed Runner Sustain success counts as useful work.
    #[arg(long, default_value_t = 5)]
    pub min_sustain_duration_minutes: u64,
    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,
    /// Return non-zero when the burn-in SLO evidence is incomplete.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Args, Debug, Clone)]
pub struct RunnerFleetAuditArgs {
    /// Expected repository that should own local dark-factory lanes for this proof.
    #[arg(long, default_value = "FlexNetOS/flexnetos_runner")]
    pub expected_repository: String,
    /// Optional JSON fixture/input of observed GitHub Actions jobs; when omitted, scan /proc.
    #[arg(long)]
    pub jobs_json: Option<PathBuf>,
    /// procfs root to scan for live GitHub Actions job environments.
    #[arg(long, default_value = "/proc")]
    pub proc_root: PathBuf,
    /// Maximum external-repository jobs allowed to occupy local runner lanes.
    #[arg(long, default_value_t = 0)]
    pub max_external_jobs: usize,
    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,
    /// Return non-zero when external lane pressure exceeds the allowed budget.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Args, Debug, Clone)]
pub struct AgenticSystemAuditArgs {
    /// Workspace root to scan.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// JSON from `gh run list --limit 1000 --json name,status,conclusion,createdAt,updatedAt,event,displayTitle,url`.
    #[arg(long)]
    pub runs_json: Option<PathBuf>,
    /// JSON from `gh pr list --state open --json state,mergedAt,statusCheckRollup,url`.
    #[arg(long)]
    pub open_prs_json: Option<PathBuf>,
    /// JSON from `gh pr list --state all --json state,mergedAt,statusCheckRollup,url`.
    #[arg(long)]
    pub prs_history_json: Option<PathBuf>,
    /// Expected repository that should own local dark-factory lanes for this proof.
    #[arg(long, default_value = "FlexNetOS/flexnetos_runner")]
    pub expected_repository: String,
    /// Optional JSON fixture/input of observed GitHub Actions jobs; when omitted, scan /proc.
    #[arg(long)]
    pub fleet_jobs_json: Option<PathBuf>,
    /// procfs root to scan for live GitHub Actions job environments.
    #[arg(long, default_value = "/proc")]
    pub proc_root: PathBuf,
    /// Minimum observed black-factor proof window in hours.
    #[arg(long, default_value_t = 12)]
    pub min_window_hours: u64,
    /// Minimum observed operations SLO burn-in window in hours.
    #[arg(long, default_value_t = 1)]
    pub min_slo_window_hours: u64,
    /// Maximum allowed observed idle gap between useful runner intervals.
    #[arg(long, default_value_t = 10)]
    pub max_idle_gap_minutes: u64,
    /// Minimum active/queued Runner Sustain backlog required at audit time.
    #[arg(long, default_value_t = 1)]
    pub min_active_or_queued_sustain: usize,
    /// Minimum successful event-driven Runner Black Factor Watch runs in the SLO window.
    #[arg(long, default_value_t = 1)]
    pub min_event_watch_wakeups: usize,
    /// Maximum failed operational workflow runs allowed in the SLO window.
    #[arg(long, default_value_t = 0)]
    pub max_failed_ops_runs: usize,
    /// Minimum duration-proven successful Runner Sustain workflow runs in the black-factor window.
    #[arg(long, default_value_t = 72)]
    pub min_sustain_runs: usize,
    /// Minimum wall-clock duration required before a Runner Sustain success counts as useful work.
    #[arg(long, default_value_t = 5)]
    pub min_sustain_duration_minutes: u64,
    /// Minimum merged PRs with clean required checks in the black-factor window.
    #[arg(long, default_value_t = 1)]
    pub min_clean_merged_prs: usize,
    /// Maximum external-repository jobs allowed to occupy local runner lanes.
    #[arg(long, default_value_t = 0)]
    pub max_external_jobs: usize,
    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,
    /// Return non-zero unless every end-to-end agentic-system proof facet is present.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Args, Debug, Clone)]
pub struct RunnerFlowAuditArgs {
    /// Workspace root to scan.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// JSON from `gh run list --json status,conclusion,name,headBranch,event,url`.
    #[arg(long)]
    pub runs_json: Option<PathBuf>,
    /// JSON from `gh pr list --json statusCheckRollup,mergeStateStatus,url`.
    #[arg(long)]
    pub prs_json: Option<PathBuf>,
    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,
    /// Return a non-zero exit when runner flow does not satisfy the local sustain contract.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Args, Debug, Clone)]
pub struct DocsDriftArgs {
    /// Workspace root to scan.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ComponentsAuditArgs {
    /// Workspace root to scan.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,
    /// Return a non-zero exit when any expected component is missing.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Args, Debug, Clone)]
pub struct TargetMiningAuditArgs {
    /// Workspace root to scan.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,
    /// Return a non-zero exit when any target lacks source, application, or guard evidence.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
pub enum CyclePhase {
    Red,
    Implement,
    Gate,
    Evaluate,
    Research,
    Upgrade,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResearchSource {
    pub id: &'static str,
    pub url: &'static str,
    pub purpose: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CodexInvocation {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalInput {
    pub red_test_first: bool,
    pub gates_passed: bool,
    pub retry_count: u8,
    pub useful_research_items: u8,
    pub runtime_secs: u64,
    pub diff_files: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvalReport {
    pub score: u8,
    pub verdict: &'static str,
    pub upgrade_allowed: bool,
    pub reasons: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DocsDriftReport {
    pub checked_features: usize,
    pub drift: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnerHealthReport {
    pub required_local_checks: Vec<String>,
    pub pending_local_checks: Vec<String>,
    pub passed_local_checks: Vec<String>,
    pub failed_local_checks: Vec<String>,
    pub missing_local_checks: Vec<String>,
    pub runner_pressure: bool,
    pub recommendation: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnerBlackFactorAuditReport {
    pub kclaw0_target: &'static str,
    pub observed_window_minutes: u64,
    pub min_window_minutes: u64,
    pub successful_sustain_runs: usize,
    pub total_duration_proven_sustain_runs: usize,
    pub min_sustain_runs: usize,
    pub remaining_sustain_runs: usize,
    pub min_minutes_to_sustain_target: u64,
    pub min_sustain_duration_minutes: u64,
    pub short_or_unproven_sustain_runs: usize,
    pub clean_merged_prs: usize,
    pub min_clean_merged_prs: usize,
    pub exceeded: bool,
    pub missing_evidence: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnerOpsSloAuditReport {
    pub kclaw0_target: &'static str,
    pub observed_window_minutes: u64,
    pub min_window_minutes: u64,
    pub max_idle_gap_minutes_observed: u64,
    pub max_idle_gap_minutes: u64,
    pub active_or_queued_sustain_runs: usize,
    pub min_active_or_queued_sustain: usize,
    pub event_watch_wakeups: usize,
    pub min_event_watch_wakeups: usize,
    pub failed_ops_runs: usize,
    pub max_failed_ops_runs: usize,
    pub open_prs: usize,
    pub queued_required_checks: usize,
    pub failed_required_checks: usize,
    pub pr_flow_seamless: bool,
    pub burn_in_ready: bool,
    pub missing_evidence: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerFleetJob {
    #[serde(default)]
    pub repository: String,
    #[serde(default)]
    pub workflow: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub job: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub head_ref: String,
    #[serde(default)]
    pub ref_name: String,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub pids: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnerFleetAuditReport {
    pub kclaw0_target: &'static str,
    pub expected_repository: String,
    pub total_jobs: usize,
    pub expected_repository_jobs: usize,
    pub external_repository_jobs: usize,
    pub max_external_jobs: usize,
    pub external_repositories: BTreeMap<String, usize>,
    pub jobs: Vec<RunnerFleetJob>,
    pub fleet_ready: bool,
    pub missing_evidence: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgenticSystemAuditReport {
    pub kclaw0_target: &'static str,
    pub components: ComponentsAuditReport,
    pub target_mining: TargetMiningAuditReport,
    pub docs_drift: DocsDriftReport,
    pub runner_flow: Option<RunnerFlowAuditReport>,
    pub runner_black_factor: Option<RunnerBlackFactorAuditReport>,
    pub runner_ops_slo: Option<RunnerOpsSloAuditReport>,
    pub runner_fleet: RunnerFleetAuditReport,
    pub research_loop_evidence: bool,
    pub evaluation_loop_evidence: bool,
    pub adaptation_loop_evidence: bool,
    pub growth_loop_evidence: bool,
    pub self_improvement_dispatch_evidence: bool,
    pub end_to_end_ready: bool,
    pub missing_evidence: Vec<&'static str>,
}

#[derive(Debug, Clone, Deserialize)]
struct PrHistoryEntry {
    #[serde(default)]
    state: String,
    #[serde(default, rename = "mergedAt")]
    merged_at: Option<String>,
    #[serde(default, rename = "statusCheckRollup")]
    status_check_rollup: Vec<CheckRollupEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnerFlowAuditReport {
    pub kclaw0_target: &'static str,
    pub sustain_workflow_present: bool,
    pub active_runs: usize,
    pub queued_runs: usize,
    pub open_prs: usize,
    pub queued_required_checks: usize,
    pub failed_required_checks: usize,
    pub idle_without_work: bool,
    pub pr_flow_seamless: bool,
    pub missing_evidence: Vec<&'static str>,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkflowRunEntry {
    #[serde(default)]
    status: String,
    #[serde(default)]
    conclusion: String,
    #[serde(default)]
    event: String,
    #[serde(default, rename = "displayTitle")]
    display_title: String,
    #[serde(default, rename = "headBranch")]
    head_branch: String,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "createdAt")]
    created_at: Option<String>,
    #[serde(default, rename = "updatedAt")]
    updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PrFlowEntry {
    #[serde(default, rename = "statusCheckRollup")]
    status_check_rollup: Vec<CheckRollupEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComponentsAuditReport {
    pub checked_components: usize,
    pub present_components: Vec<String>,
    pub missing_components: Vec<String>,
    pub components: Vec<LoopComponentStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoopComponentStatus {
    pub id: &'static str,
    pub surface: &'static str,
    pub path: &'static str,
    pub present: bool,
    pub rationale: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetMiningAuditReport {
    pub checked_targets: usize,
    pub covered_targets: Vec<String>,
    pub missing_targets: Vec<String>,
    pub targets: Vec<TargetMiningStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetMiningStatus {
    pub id: &'static str,
    pub url: &'static str,
    pub source_evidence: bool,
    pub application_evidence: bool,
    pub guard_evidence: bool,
    pub missing: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetMiningTarget {
    id: &'static str,
    url: &'static str,
    source_terms: &'static [&'static str],
    application_terms: &'static [(&'static str, &'static str)],
    guard_terms: &'static [(&'static str, &'static str)],
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LoopComponent {
    id: &'static str,
    surface: &'static str,
    path: &'static str,
    rationale: &'static str,
}

#[derive(Debug, Clone, Deserialize)]
struct CheckRollupPayload {
    #[serde(default, rename = "statusCheckRollup")]
    status_check_rollup: Vec<CheckRollupEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct CheckRollupEntry {
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    conclusion: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CycleManifest {
    #[serde(default = "default_cycle_manifest_schema_version")]
    pub schema_version: u8,
    pub goal: String,
    pub pr_title: String,
    pub prompt_sha256: String,
    pub once: bool,
    pub auto_merge: bool,
    pub strict_upgrade_only: bool,
    pub phases: Vec<CyclePhase>,
}

#[derive(Debug, Clone, Serialize)]
struct CycleEvent<'a> {
    event: &'a str,
    phase: CyclePhase,
    detail: &'a str,
}

pub fn execute(cmd: ForgeLoopCommand) -> Result<()> {
    match cmd {
        ForgeLoopCommand::Run(args) => run(args),
        ForgeLoopCommand::Eval(args) => eval(args),
        ForgeLoopCommand::Research(args) => research(args),
        ForgeLoopCommand::SelfUpgrade(args) => self_upgrade(args),
        ForgeLoopCommand::Doctor(args) => doctor(args),
        ForgeLoopCommand::RunnerHealth(args) => runner_health(args),
        ForgeLoopCommand::RunnerFlowAudit(args) => runner_flow_audit(args),
        ForgeLoopCommand::RunnerBlackFactorAudit(args) => runner_black_factor_audit(args),
        ForgeLoopCommand::RunnerOpsSloAudit(args) => runner_ops_slo_audit(args),
        ForgeLoopCommand::RunnerFleetAudit(args) => runner_fleet_audit(args),
        ForgeLoopCommand::AgenticSystemAudit(args) => agentic_system_audit(args),
        ForgeLoopCommand::DocsDrift(args) => docs_drift(args),
        ForgeLoopCommand::ComponentsAudit(args) => components_audit(args),
        ForgeLoopCommand::TargetMiningAudit(args) => target_mining_audit(args),
    }
}

fn run(args: RunArgs) -> Result<()> {
    if !args.once {
        return Err(anyhow!(
            "once must be true; forge-loop run executes exactly one supervised cycle"
        ));
    }

    let cycle_dir = args.out.join(timestamp_label()?);
    fs::create_dir_all(&cycle_dir)
        .with_context(|| format!("create forge-loop artifact dir {}", cycle_dir.display()))?;
    fs::write(
        cycle_dir.join("cycle-manifest.json"),
        serde_json::to_string_pretty(&cycle_manifest(&args))?,
    )?;
    fs::write(
        cycle_dir.join("research-sources.json"),
        serde_json::to_string_pretty(&research_sources())?,
    )?;
    let log = cycle_dir.join("events.jsonl");
    append_event(
        &log,
        CycleEvent {
            event: "cycle.started",
            phase: CyclePhase::Red,
            detail: "forge-loop TDD cycle started",
        },
    )?;

    let prompt = cycle_prompt(&args.goal, args.auto_merge);
    let invocation = codex_invocation(prompt);
    fs::write(
        cycle_dir.join("codex-invocation.json"),
        serde_json::to_string_pretty(&invocation)?,
    )?;

    if args.dry_run {
        append_event(
            &log,
            CycleEvent {
                event: "cycle.dry_run",
                phase: CyclePhase::Implement,
                detail: "codex invocation planned but not executed",
            },
        )?;
        let report = evaluate(EvalInput::fixture());
        fs::write(
            cycle_dir.join("evaluation.json"),
            serde_json::to_string_pretty(&report)?,
        )?;
        println!("forge-loop dry run complete: {}", cycle_dir.display());
        println!("score={} verdict={}", report.score, report.verdict);
        return Ok(());
    }

    let status = Command::new(&invocation.program)
        .args(&invocation.args)
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("spawn Codex CLI {}", invocation.program))?;
    if !status.success() {
        append_event(
            &log,
            CycleEvent {
                event: "codex.failed",
                phase: CyclePhase::Implement,
                detail:
                    "codex exec returned non-zero; inspect transcript/stdout in Codex session logs",
            },
        )?;
        return Err(anyhow!("codex exec failed with status {status}"));
    }

    append_event(
        &log,
        CycleEvent {
            event: "cycle.codex_complete",
            phase: CyclePhase::Gate,
            detail: "codex implementation completed; run repository gates next",
        },
    )?;
    println!("forge-loop cycle complete: {}", cycle_dir.display());
    Ok(())
}

fn eval(args: EvalArgs) -> Result<()> {
    let input = if args.fixture {
        EvalInput::fixture()
    } else if let Some(path) = args.metrics {
        parse_eval_metrics(&path)?
    } else {
        return Err(anyhow!("provide --fixture or --metrics <path>"));
    };
    if let Some(path) = args.manifest {
        parse_cycle_manifest(&path)?;
    }
    let report = evaluate(input);
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn research(args: ResearchArgs) -> Result<()> {
    let sources = research_sources();
    if args.dry_run {
        println!("{}", serde_json::to_string_pretty(&sources)?);
        return Ok(());
    }
    let prompt = research_prompt(&args.focus, &sources);
    let invocation = codex_invocation(prompt);
    println!("{}", serde_json::to_string_pretty(&invocation)?);
    let status = Command::new(&invocation.program)
        .args(&invocation.args)
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("spawn Codex CLI {}", invocation.program))?;
    if !status.success() {
        return Err(anyhow!("codex research agent failed with status {status}"));
    }
    Ok(())
}

fn self_upgrade(args: SelfUpgradeArgs) -> Result<()> {
    let report = evaluate(EvalInput::fixture());
    let allowed = report.score >= args.min_score && report.upgrade_allowed;
    let plan = serde_json::json!({
        "score": report.score,
        "min_score": args.min_score,
        "allowed": allowed,
        "branch_prefix": "codex/forge-loop-self-upgrade",
        "merge_policy": "auto-merge green when repository settings allow; otherwise merge after green checks",
        "strict_upgrade_only": true,
        "runner_health_input": "gh pr view <PR> --json statusCheckRollup",
        "required_local_checks": REQUIRED_LOCAL_CHECKS,
        "required_gate_commands": REQUIRED_GATE_COMMANDS,
        "components_audit": "fxrun forge-loop components-audit --json"
    });
    println!("{}", serde_json::to_string_pretty(&plan)?);
    if args.dry_run || !allowed {
        return Ok(());
    }
    let prompt = format!(
        "You are the forge-loop self-upgrade agent. Implement exactly one small, TDD-first reliability, accuracy, or speed improvement for fxrun forge-loop. Commit, push, open a PR, and enable auto-merge if checks are green. Evaluation score: {}. Strict upgrade only; no downgrades/removals without parity proof.",
        report.score
    );
    let invocation = codex_invocation(prompt);
    let status = Command::new(&invocation.program)
        .args(&invocation.args)
        .status()?;
    if !status.success() {
        return Err(anyhow!("codex self-upgrade failed with status {status}"));
    }
    Ok(())
}

fn doctor(args: DoctorArgs) -> Result<()> {
    let report = serde_json::json!({
        "codex": codex_program(),
        "artifact_root": DEFAULT_ARTIFACT_ROOT,
        "research_sources": research_sources(),
        "phases": ["red", "implement", "gate", "evaluate", "research", "upgrade"],
        "auto_merge_green": true,
        "strict_upgrade_only": true,
        "runner_health_input": "gh pr view <PR> --json statusCheckRollup",
        "required_local_checks": REQUIRED_LOCAL_CHECKS,
        "required_gate_commands": REQUIRED_GATE_COMMANDS,
        "target_mining_audit": "fxrun forge-loop target-mining-audit --json",
        "runner_flow_audit": "fxrun forge-loop runner-flow-audit --json",
        "runner_black_factor_audit": "fxrun forge-loop runner-black-factor-audit --json",
        "runner_ops_slo_audit": "fxrun forge-loop runner-ops-slo-audit --json",
        "runner_fleet_audit": "fxrun forge-loop runner-fleet-audit --json",
        "agentic_system_audit": "fxrun forge-loop agentic-system-audit --json"
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("fxrun forge-loop");
        println!("  codex cli          : {}", codex_program());
        println!("  artifact root      : {DEFAULT_ARTIFACT_ROOT}");
        println!("  phases             : red → implement → gate → evaluate → research → upgrade");
        println!("  auto-merge policy  : green PRs when repository settings allow");
        println!("  strict upgrade     : enabled");
        println!("  required gates     :");
        for command in REQUIRED_GATE_COMMANDS {
            println!("    - {command}");
        }
        println!("  runner health      : use `fxrun forge-loop runner-health --checks-json <gh-pr-view.json>`");
        println!("  runner flow        : use `fxrun forge-loop runner-flow-audit --json`");
        println!("  black-factor proof : use `fxrun forge-loop runner-black-factor-audit --json`");
        println!("  ops SLO burn-in    : use `fxrun forge-loop runner-ops-slo-audit --json`");
        println!("  fleet lane audit   : use `fxrun forge-loop runner-fleet-audit --json`");
        println!("  agentic system     : use `fxrun forge-loop agentic-system-audit --json`");
        println!("  component audit    : use `fxrun forge-loop components-audit --json`");
        println!("  target mining      : use `fxrun forge-loop target-mining-audit --json`");
        println!(
            "  required checks    : {}",
            REQUIRED_LOCAL_CHECKS.join(", ")
        );
        println!("  research sources   :");
        for source in research_sources() {
            println!("    - {} ({})", source.id, source.url);
        }
    }
    Ok(())
}

fn runner_health(args: RunnerHealthArgs) -> Result<()> {
    let report = runner_health_report(&args.checks_json)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("fxrun forge-loop runner health");
        if report.runner_pressure {
            println!("  runner pressure    : detected");
            println!("  pending local checks:");
            for check in &report.pending_local_checks {
                println!("    - {check}");
            }
        } else {
            println!("  runner pressure    : clear");
        }
        if !report.missing_local_checks.is_empty() {
            println!("  missing local checks:");
            for check in &report.missing_local_checks {
                println!("    - {check}");
            }
        }
        println!("  recommendation     : {}", report.recommendation);
    }
    Ok(())
}

fn runner_flow_audit(args: RunnerFlowAuditArgs) -> Result<()> {
    let report = runner_flow_audit_report(&args)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("fxrun forge-loop runner flow audit");
        println!("  kclaw0 target       : {}", report.kclaw0_target);
        println!(
            "  sustain workflow    : {}",
            report.sustain_workflow_present
        );
        println!("  active runs         : {}", report.active_runs);
        println!("  queued runs         : {}", report.queued_runs);
        println!("  open PRs            : {}", report.open_prs);
        println!("  queued required     : {}", report.queued_required_checks);
        println!("  failed required     : {}", report.failed_required_checks);
        println!("  idle without work   : {}", report.idle_without_work);
        println!("  PR flow seamless    : {}", report.pr_flow_seamless);
        if !report.missing_evidence.is_empty() {
            println!("  missing evidence    :");
            for item in &report.missing_evidence {
                println!("    - {item}");
            }
        }
    }

    if args.strict && !report.missing_evidence.is_empty() {
        Err(anyhow!(
            "runner flow audit missing evidence: {}",
            report.missing_evidence.join(", ")
        ))
    } else {
        Ok(())
    }
}

fn runner_black_factor_audit(args: RunnerBlackFactorAuditArgs) -> Result<()> {
    let report = runner_black_factor_audit_report(&args)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("fxrun forge-loop runner black-factor audit");
        println!("  kclaw0 target        : {}", report.kclaw0_target);
        println!(
            "  observed window min  : {}",
            report.observed_window_minutes
        );
        println!("  required window min  : {}", report.min_window_minutes);
        println!(
            "  sustain runs in win  : {}",
            report.successful_sustain_runs
        );
        println!(
            "  total proven sustain : {}",
            report.total_duration_proven_sustain_runs
        );
        println!("  required sustain     : {}", report.min_sustain_runs);
        println!("  remaining sustain    : {}", report.remaining_sustain_runs);
        println!(
            "  min minutes to target: {}",
            report.min_minutes_to_sustain_target
        );
        println!(
            "  min sustain duration : {} minutes",
            report.min_sustain_duration_minutes
        );
        println!(
            "  short/unproven runs  : {}",
            report.short_or_unproven_sustain_runs
        );
        println!("  clean merged PRs     : {}", report.clean_merged_prs);
        println!("  required clean PRs   : {}", report.min_clean_merged_prs);
        println!("  exceeded             : {}", report.exceeded);
        if !report.missing_evidence.is_empty() {
            println!("  missing evidence     :");
            for item in &report.missing_evidence {
                println!("    - {item}");
            }
        }
    }

    if args.strict && !report.exceeded {
        Err(anyhow!(
            "runner black-factor target not exceeded: {}",
            report.missing_evidence.join(", ")
        ))
    } else {
        Ok(())
    }
}

fn runner_ops_slo_audit(args: RunnerOpsSloAuditArgs) -> Result<()> {
    let report = runner_ops_slo_audit_report(&args)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("fxrun forge-loop runner ops SLO audit");
        println!("  kclaw0 target        : {}", report.kclaw0_target);
        println!(
            "  observed window min  : {}",
            report.observed_window_minutes
        );
        println!("  required window min  : {}", report.min_window_minutes);
        println!(
            "  max idle gap min     : {}",
            report.max_idle_gap_minutes_observed
        );
        println!("  allowed idle gap min : {}", report.max_idle_gap_minutes);
        println!(
            "  active/queued sustain: {}",
            report.active_or_queued_sustain_runs
        );
        println!("  event watch wakeups  : {}", report.event_watch_wakeups);
        println!("  failed ops runs      : {}", report.failed_ops_runs);
        println!("  open PRs             : {}", report.open_prs);
        println!("  queued required      : {}", report.queued_required_checks);
        println!("  failed required      : {}", report.failed_required_checks);
        println!("  PR flow seamless     : {}", report.pr_flow_seamless);
        println!("  burn-in ready        : {}", report.burn_in_ready);
        if !report.missing_evidence.is_empty() {
            println!("  missing evidence     :");
            for item in &report.missing_evidence {
                println!("    - {item}");
            }
        }
    }

    if args.strict && !report.burn_in_ready {
        Err(anyhow!(
            "runner ops SLO evidence incomplete: {}",
            report.missing_evidence.join(", ")
        ))
    } else {
        Ok(())
    }
}

fn runner_fleet_audit(args: RunnerFleetAuditArgs) -> Result<()> {
    let report = runner_fleet_audit_report(&args)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("fxrun forge-loop runner fleet audit");
        println!("  kclaw0 target       : {}", report.kclaw0_target);
        println!("  expected repository : {}", report.expected_repository);
        println!("  total jobs          : {}", report.total_jobs);
        println!(
            "  expected repo jobs  : {}",
            report.expected_repository_jobs
        );
        println!(
            "  external repo jobs  : {}",
            report.external_repository_jobs
        );
        println!("  max external jobs   : {}", report.max_external_jobs);
        if !report.external_repositories.is_empty() {
            println!("  external repositories:");
            for (repo, count) in &report.external_repositories {
                println!("    - {repo}: {count}");
            }
        }
        println!("  fleet ready         : {}", report.fleet_ready);
        if !report.missing_evidence.is_empty() {
            println!("  missing evidence    :");
            for item in &report.missing_evidence {
                println!("    - {item}");
            }
        }
    }

    if args.strict && !report.fleet_ready {
        Err(anyhow!(
            "runner fleet audit evidence incomplete: {}",
            report.missing_evidence.join(", ")
        ))
    } else {
        Ok(())
    }
}

fn agentic_system_audit(args: AgenticSystemAuditArgs) -> Result<()> {
    let report = agentic_system_audit_report(&args)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("fxrun forge-loop agentic system audit");
        println!("  kclaw0 target        : {}", report.kclaw0_target);
        println!(
            "  component inventory  : {}/{}",
            report.components.present_components.len(),
            report.components.checked_components
        );
        println!(
            "  target mining        : {}/{}",
            report.target_mining.covered_targets.len(),
            report.target_mining.checked_targets
        );
        println!("  docs drift           : {}", report.docs_drift.drift.len());
        println!("  always researching   : {}", report.research_loop_evidence);
        println!(
            "  always evaluating    : {}",
            report.evaluation_loop_evidence
        );
        println!(
            "  always adapting      : {}",
            report.adaptation_loop_evidence
        );
        println!("  always growing       : {}", report.growth_loop_evidence);
        println!(
            "  self-improvement    : {}",
            report.self_improvement_dispatch_evidence
        );
        println!("  runner flow          : {}", report.runner_flow.is_some());
        println!(
            "  black-factor proof   : {}",
            report
                .runner_black_factor
                .as_ref()
                .is_some_and(|runner| runner.exceeded)
        );
        println!(
            "  ops burn-in proof    : {}",
            report
                .runner_ops_slo
                .as_ref()
                .is_some_and(|ops| ops.burn_in_ready)
        );
        println!(
            "  fleet ready          : {}",
            report.runner_fleet.fleet_ready
        );
        println!("  end-to-end ready     : {}", report.end_to_end_ready);
        if !report.missing_evidence.is_empty() {
            println!("  missing evidence     :");
            for item in &report.missing_evidence {
                println!("    - {item}");
            }
        }
    }

    if args.strict && !report.end_to_end_ready {
        Err(anyhow!(
            "agentic system evidence incomplete: {}",
            report.missing_evidence.join(", ")
        ))
    } else {
        Ok(())
    }
}

fn runner_ops_slo_audit_report(args: &RunnerOpsSloAuditArgs) -> Result<RunnerOpsSloAuditReport> {
    let runs = parse_json_vec::<WorkflowRunEntry>(&args.runs_json)?;
    let prs = parse_json_vec::<PrFlowEntry>(&args.prs_json)?;

    let timestamps = runs
        .iter()
        .filter_map(|run| run.created_at.as_deref())
        .filter_map(parse_rfc3339_utc_seconds)
        .collect::<Vec<_>>();
    let (observed_start, observed_end) = match (timestamps.iter().min(), timestamps.iter().max()) {
        (Some(first), Some(last)) if last >= first => (*first, *last),
        _ => (0, 0),
    };
    let observed_window_minutes = if observed_end >= observed_start {
        ((observed_end - observed_start) / 60) as u64
    } else {
        0
    };
    let min_window_minutes = args.min_window_hours.saturating_mul(60);
    let proof_window_start = observed_end.saturating_sub((min_window_minutes as i64) * 60);

    let run_in_window = |run: &WorkflowRunEntry| {
        run.created_at
            .as_deref()
            .and_then(parse_rfc3339_utc_seconds)
            .is_some_and(|created| created >= proof_window_start)
    };

    let active_or_queued_sustain_runs = runs
        .iter()
        .filter(|run| run.name == "Runner Sustain")
        .filter(|run| {
            run.status.eq_ignore_ascii_case("queued")
                || run.status.eq_ignore_ascii_case("pending")
                || run.status.eq_ignore_ascii_case("in_progress")
        })
        .count();

    let event_watch_wakeups = runs
        .iter()
        .filter(|run| is_successful_runner_watch_rehydration(run))
        .filter(|run| run_in_window(run))
        .count();

    let failed_ops_runs = runs
        .iter()
        .filter(|run| run_in_window(run))
        .filter(|run| is_ops_workflow(&run.name))
        .filter(|run| run.status.eq_ignore_ascii_case("completed"))
        .filter(|run| is_failed_ops_run(run, &runs))
        .count();

    let mut queued_required_checks = 0;
    let mut failed_required_checks = 0;
    for pr in &prs {
        let runner_health = classify_runner_health(&pr.status_check_rollup);
        queued_required_checks += runner_health.pending_local_checks.len();
        failed_required_checks += runner_health.failed_local_checks.len();
    }
    let open_prs = prs.len();
    let pr_flow_seamless =
        open_prs == 0 || (queued_required_checks == 0 && failed_required_checks == 0);

    let max_idle_gap_minutes_observed = max_local_runner_idle_gap_minutes(
        &runs,
        proof_window_start,
        observed_end,
        args.min_sustain_duration_minutes,
    );

    let mut missing_evidence = Vec::new();
    if observed_window_minutes < min_window_minutes {
        missing_evidence.push("observed_slo_window");
    }
    if max_idle_gap_minutes_observed > args.max_idle_gap_minutes {
        missing_evidence.push("idle_gap_slo");
    }
    if active_or_queued_sustain_runs < args.min_active_or_queued_sustain {
        missing_evidence.push("active_or_queued_sustain_backlog");
    }
    if event_watch_wakeups < args.min_event_watch_wakeups {
        missing_evidence.push("event_watch_rehydration");
    }
    if failed_ops_runs > args.max_failed_ops_runs {
        missing_evidence.push("failed_ops_budget");
    }
    if !pr_flow_seamless {
        missing_evidence.push("seamless_pr_flow");
    }
    let burn_in_ready = missing_evidence.is_empty();

    Ok(RunnerOpsSloAuditReport {
        kclaw0_target: "unattended dark-factory operations burn-in with bounded idle gaps, event-driven rehydration, clean PR flow, and zero failed operational runs",
        observed_window_minutes,
        min_window_minutes,
        max_idle_gap_minutes_observed,
        max_idle_gap_minutes: args.max_idle_gap_minutes,
        active_or_queued_sustain_runs,
        min_active_or_queued_sustain: args.min_active_or_queued_sustain,
        event_watch_wakeups,
        min_event_watch_wakeups: args.min_event_watch_wakeups,
        failed_ops_runs,
        max_failed_ops_runs: args.max_failed_ops_runs,
        open_prs,
        queued_required_checks,
        failed_required_checks,
        pr_flow_seamless,
        burn_in_ready,
        missing_evidence,
    })
}

fn runner_fleet_audit_report(args: &RunnerFleetAuditArgs) -> Result<RunnerFleetAuditReport> {
    let jobs = if let Some(path) = &args.jobs_json {
        parse_json_vec::<RunnerFleetJob>(path)?
    } else {
        scan_proc_for_runner_jobs(&args.proc_root)?
    };
    let jobs = dedupe_runner_fleet_jobs(jobs);
    let expected_repository_jobs = jobs
        .iter()
        .filter(|job| job.repository == args.expected_repository)
        .count();
    let mut external_repositories = BTreeMap::new();
    for job in jobs
        .iter()
        .filter(|job| job.repository != args.expected_repository)
    {
        *external_repositories
            .entry(job.repository.clone())
            .or_insert(0) += 1;
    }
    let external_repository_jobs = jobs.len().saturating_sub(expected_repository_jobs);
    let mut missing_evidence = Vec::new();
    if external_repository_jobs > args.max_external_jobs {
        missing_evidence.push("external_runner_lane_pressure");
    }
    let fleet_ready = missing_evidence.is_empty();

    Ok(RunnerFleetAuditReport {
        kclaw0_target: "local self-hosted runner lanes are attributable and not invisibly occupied by cross-repo work during dark-factory proof",
        expected_repository: args.expected_repository.clone(),
        total_jobs: jobs.len(),
        expected_repository_jobs,
        external_repository_jobs,
        max_external_jobs: args.max_external_jobs,
        external_repositories,
        jobs,
        fleet_ready,
        missing_evidence,
    })
}

fn agentic_system_audit_report(args: &AgenticSystemAuditArgs) -> Result<AgenticSystemAuditReport> {
    let components = components_audit_report(&args.root);
    let target_mining = target_mining_audit_report(&args.root);
    let docs_drift = docs_drift_report(&args.root)?;

    let runner_flow = match (&args.runs_json, &args.open_prs_json) {
        (Some(runs_json), Some(prs_json)) => {
            Some(runner_flow_audit_report(&RunnerFlowAuditArgs {
                root: args.root.clone(),
                runs_json: Some(runs_json.clone()),
                prs_json: Some(prs_json.clone()),
                json: true,
                strict: false,
            })?)
        }
        _ => None,
    };
    let runner_black_factor = match (&args.runs_json, &args.prs_history_json) {
        (Some(runs_json), Some(prs_json)) => Some(runner_black_factor_audit_report(
            &RunnerBlackFactorAuditArgs {
                runs_json: runs_json.clone(),
                prs_json: prs_json.clone(),
                min_window_hours: args.min_window_hours,
                min_sustain_runs: args.min_sustain_runs,
                min_sustain_duration_minutes: args.min_sustain_duration_minutes,
                min_clean_merged_prs: args.min_clean_merged_prs,
                json: true,
                strict: false,
            },
        )?),
        _ => None,
    };
    let runner_ops_slo = match (&args.runs_json, &args.open_prs_json) {
        (Some(runs_json), Some(prs_json)) => {
            Some(runner_ops_slo_audit_report(&RunnerOpsSloAuditArgs {
                runs_json: runs_json.clone(),
                prs_json: prs_json.clone(),
                min_window_hours: args.min_slo_window_hours,
                max_idle_gap_minutes: args.max_idle_gap_minutes,
                min_active_or_queued_sustain: args.min_active_or_queued_sustain,
                min_event_watch_wakeups: args.min_event_watch_wakeups,
                max_failed_ops_runs: args.max_failed_ops_runs,
                min_sustain_duration_minutes: args.min_sustain_duration_minutes,
                json: true,
                strict: false,
            })?)
        }
        _ => None,
    };
    let runner_fleet = runner_fleet_audit_report(&RunnerFleetAuditArgs {
        expected_repository: args.expected_repository.clone(),
        jobs_json: args.fleet_jobs_json.clone(),
        proc_root: args.proc_root.clone(),
        max_external_jobs: args.max_external_jobs,
        json: true,
        strict: false,
    })?;

    let research_loop_evidence = target_mining.missing_targets.is_empty()
        && target_mining
            .covered_targets
            .iter()
            .any(|target| target == "kclaw0")
        && target_mining
            .covered_targets
            .iter()
            .any(|target| target == "kclaw0-referenced-resources")
        && all_file_terms_present(
            &args.root,
            &[
                (
                    ".agents/skills/forge-loop-research/SKILL.md",
                    "Required sources",
                ),
                (
                    "docs/forge-loop/codex-target-mining.md",
                    "kclaw0 referenced resources",
                ),
            ],
        );
    let evaluation_loop_evidence = components.missing_components.is_empty()
        && docs_drift.drift.is_empty()
        && all_file_terms_present(
            &args.root,
            &[
                (
                    ".codex/checklists/forge-loop-cycle.toml",
                    "target_mining_audit",
                ),
                (".codex/checklists/forge-loop-cycle.toml", "docs_drift"),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "agentic_system_audit_report",
                ),
            ],
        );
    let adaptation_loop_evidence = all_file_terms_present(
        &args.root,
        &[
            ("docs/kclaw0-upgrade-ledger.md", "self-upgrade"),
            (
                "docs/kclaw0-upgrade-ledger.md",
                "kclaw0 target-mining proof",
            ),
            (
                "docs/kclaw0-upgrade-ledger.md",
                "kclaw0 referenced-resource proof",
            ),
        ],
    );
    let growth_loop_evidence = runner_black_factor.as_ref().is_some_and(|black| {
        black.exceeded && black.clean_merged_prs >= black.min_clean_merged_prs
    }) && runner_ops_slo.as_ref().is_some_and(|ops| ops.burn_in_ready)
        && runner_flow
            .as_ref()
            .is_some_and(|flow| flow.pr_flow_seamless && !flow.idle_without_work);
    let self_improvement_dispatch_evidence = all_file_terms_present(
        &args.root,
        &[
            (
                ".github/workflows/agentic-system-watch.yml",
                "name: Agentic System Watch",
            ),
            (
                ".github/workflows/agentic-system-watch.yml",
                "agentic-system-audit",
            ),
            (
                ".github/workflows/agentic-system-watch.yml",
                "gh workflow run codex-forge-loop.yml",
            ),
            (
                ".github/workflows/codex-forge-loop.yml",
                "openai/codex-action@v1",
            ),
            (".github/workflows/codex-forge-loop.yml", "CODEX_HOME"),
            (".github/workflows/codex-forge-loop.yml", "codex exec"),
        ],
    );

    let mut missing_evidence = Vec::new();
    if !components.missing_components.is_empty() {
        missing_evidence.push("component_inventory");
    }
    if !target_mining.missing_targets.is_empty() {
        missing_evidence.push("target_mining");
    }
    if !docs_drift.drift.is_empty() {
        missing_evidence.push("docs_drift");
    }
    if !research_loop_evidence {
        missing_evidence.push("always_researching");
    }
    if !evaluation_loop_evidence {
        missing_evidence.push("always_evaluating");
    }
    if !adaptation_loop_evidence {
        missing_evidence.push("always_adapting");
    }
    if runner_flow.is_none() {
        missing_evidence.push("runner_flow_live_evidence");
    } else if runner_flow
        .as_ref()
        .is_some_and(|flow| !flow.missing_evidence.is_empty())
    {
        missing_evidence.push("runner_flow");
    }
    if runner_black_factor.is_none() {
        missing_evidence.push("black_factor_live_evidence");
    } else if runner_black_factor
        .as_ref()
        .is_some_and(|black| !black.exceeded)
    {
        missing_evidence.push("black_factor_exceeded");
    }
    if runner_ops_slo.is_none() {
        missing_evidence.push("ops_slo_live_evidence");
    } else if runner_ops_slo
        .as_ref()
        .is_some_and(|ops| !ops.burn_in_ready)
    {
        missing_evidence.push("ops_slo_burn_in");
    }
    if !runner_fleet.fleet_ready {
        missing_evidence.push("fleet_lane_ownership");
    }
    if !growth_loop_evidence {
        missing_evidence.push("always_growing");
    }
    if !self_improvement_dispatch_evidence {
        missing_evidence.push("self_improvement_dispatch");
    }

    let end_to_end_ready = missing_evidence.is_empty();
    Ok(AgenticSystemAuditReport {
        kclaw0_target: "end-to-end 24/7 agentic system that is always researching, evaluating, adapting, growing, and improving while runners and PRs flow",
        components,
        target_mining,
        docs_drift,
        runner_flow,
        runner_black_factor,
        runner_ops_slo,
        runner_fleet,
        research_loop_evidence,
        evaluation_loop_evidence,
        adaptation_loop_evidence,
        growth_loop_evidence,
        self_improvement_dispatch_evidence,
        end_to_end_ready,
        missing_evidence,
    })
}

fn scan_proc_for_runner_jobs(proc_root: &Path) -> Result<Vec<RunnerFleetJob>> {
    let mut jobs = Vec::new();
    let proc_index = proc_index(proc_root)?;
    let entries = fs::read_dir(proc_root)
        .with_context(|| format!("read proc root {}", proc_root.display()))?;
    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(pid) = file_name.to_string_lossy().parse::<u32>().ok() else {
            continue;
        };
        if !has_runner_worker_ancestor(pid, &proc_index) {
            continue;
        }
        let environ_path = entry.path().join("environ");
        let Ok(environ) = fs::read(&environ_path) else {
            continue;
        };
        let env = parse_nul_env(&environ);
        let Some(repository) = env.get("GITHUB_REPOSITORY").cloned() else {
            continue;
        };
        if repository.is_empty() {
            continue;
        }
        jobs.push(RunnerFleetJob {
            repository,
            workflow: env.get("GITHUB_WORKFLOW").cloned().unwrap_or_default(),
            run_id: env.get("GITHUB_RUN_ID").cloned().unwrap_or_default(),
            job: env.get("GITHUB_JOB").cloned().unwrap_or_default(),
            action: env.get("GITHUB_ACTION").cloned().unwrap_or_default(),
            head_ref: env.get("GITHUB_HEAD_REF").cloned().unwrap_or_default(),
            ref_name: env.get("GITHUB_REF_NAME").cloned().unwrap_or_default(),
            workspace: env.get("GITHUB_WORKSPACE").cloned().unwrap_or_default(),
            pids: vec![pid],
        });
    }
    Ok(jobs)
}

#[derive(Debug, Clone)]
struct ProcInfo {
    ppid: u32,
    cmdline: String,
}

fn proc_index(proc_root: &Path) -> Result<BTreeMap<u32, ProcInfo>> {
    let mut index = BTreeMap::new();
    let entries = fs::read_dir(proc_root)
        .with_context(|| format!("read proc root {}", proc_root.display()))?;
    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(pid) = file_name.to_string_lossy().parse::<u32>().ok() else {
            continue;
        };
        let ppid = fs::read_to_string(entry.path().join("status"))
            .ok()
            .and_then(|status| parse_status_ppid(&status))
            .unwrap_or(0);
        let cmdline = fs::read(entry.path().join("cmdline"))
            .ok()
            .map(|raw| {
                String::from_utf8_lossy(&raw)
                    .replace('\0', " ")
                    .trim()
                    .to_string()
            })
            .filter(|cmd| !cmd.is_empty())
            .or_else(|| {
                fs::read_to_string(entry.path().join("comm"))
                    .ok()
                    .map(|comm| comm.trim().to_string())
            })
            .unwrap_or_default();
        index.insert(pid, ProcInfo { ppid, cmdline });
    }
    Ok(index)
}

fn parse_status_ppid(status: &str) -> Option<u32> {
    status.lines().find_map(|line| {
        let value = line.strip_prefix("PPid:")?;
        value.trim().parse::<u32>().ok()
    })
}

fn has_runner_worker_ancestor(pid: u32, index: &BTreeMap<u32, ProcInfo>) -> bool {
    let mut current = Some(pid);
    let mut depth = 0;
    while let Some(candidate) = current {
        depth += 1;
        if depth > 64 {
            return false;
        }
        let Some(info) = index.get(&candidate) else {
            return false;
        };
        if info.cmdline.contains("Runner.Worker") {
            return true;
        }
        if info.ppid == 0 || info.ppid == candidate {
            return false;
        }
        current = Some(info.ppid);
    }
    false
}

fn parse_nul_env(raw: &[u8]) -> BTreeMap<String, String> {
    raw.split(|byte| *byte == 0)
        .filter_map(|entry| {
            if entry.is_empty() {
                return None;
            }
            let text = String::from_utf8_lossy(entry);
            let (key, value) = text.split_once('=')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

fn dedupe_runner_fleet_jobs(jobs: Vec<RunnerFleetJob>) -> Vec<RunnerFleetJob> {
    let mut by_key: BTreeMap<(String, String, String, String), RunnerFleetJob> = BTreeMap::new();
    for mut job in jobs {
        let key = (
            job.repository.clone(),
            job.run_id.clone(),
            job.job.clone(),
            job.workspace.clone(),
        );
        by_key
            .entry(key)
            .and_modify(|existing| {
                existing.pids.append(&mut job.pids);
                existing.pids.sort_unstable();
                existing.pids.dedup();
            })
            .or_insert(job);
    }
    by_key.into_values().collect()
}

fn runner_black_factor_audit_report(
    args: &RunnerBlackFactorAuditArgs,
) -> Result<RunnerBlackFactorAuditReport> {
    let runs = parse_json_vec::<WorkflowRunEntry>(&args.runs_json)?;
    let prs = parse_json_vec::<PrHistoryEntry>(&args.prs_json)?;

    let timestamps = runs
        .iter()
        .filter_map(|run| run.created_at.as_deref())
        .filter_map(parse_rfc3339_utc_seconds)
        .collect::<Vec<_>>();
    let observed_window_minutes = match (timestamps.iter().min(), timestamps.iter().max()) {
        (Some(first), Some(last)) if last >= first => ((*last - *first) / 60) as u64,
        _ => 0,
    };
    let min_window_minutes = args.min_window_hours.saturating_mul(60);
    let proof_window_start = timestamps
        .iter()
        .max()
        .map(|last| last.saturating_sub((min_window_minutes as i64) * 60));

    let total_duration_proven_sustain_runs = runs
        .iter()
        .filter(|run| {
            runner_sustain_duration_minutes(run, args.min_sustain_duration_minutes).is_some()
        })
        .count();
    let successful_sustain_runs = runs
        .iter()
        .filter(|run| {
            runner_sustain_duration_minutes(run, args.min_sustain_duration_minutes).is_some()
                && proof_window_start
                    .zip(
                        run.created_at
                            .as_deref()
                            .and_then(parse_rfc3339_utc_seconds),
                    )
                    .is_some_and(|(window_start, created)| created >= window_start)
        })
        .count();
    let short_or_unproven_sustain_runs = runs
        .iter()
        .filter(|run| is_successful_runner_sustain(run))
        .filter(|run| {
            runner_sustain_duration_minutes(run, args.min_sustain_duration_minutes).is_none()
        })
        .count();

    let remaining_sustain_runs = args
        .min_sustain_runs
        .saturating_sub(successful_sustain_runs);
    let min_minutes_to_sustain_target =
        (remaining_sustain_runs as u64).saturating_mul(args.min_sustain_duration_minutes);

    let clean_merged_prs = prs
        .iter()
        .filter(|pr| {
            pr.state.eq_ignore_ascii_case("MERGED")
                && pr.merged_at.is_some()
                && classify_runner_health(&pr.status_check_rollup)
                    .failed_local_checks
                    .is_empty()
        })
        .count();

    let mut missing_evidence = Vec::new();
    if observed_window_minutes < min_window_minutes {
        missing_evidence.push("observed_12h_window");
    }
    if successful_sustain_runs < args.min_sustain_runs {
        missing_evidence.push("sustain_run_count");
    }
    if clean_merged_prs < args.min_clean_merged_prs {
        missing_evidence.push("clean_merged_pr_flow");
    }
    let exceeded = missing_evidence.is_empty();

    Ok(RunnerBlackFactorAuditReport {
        kclaw0_target: "12+ hour dark-factory persistence with repeated useful runner sustain runs and green-gated PR flow",
        observed_window_minutes,
        min_window_minutes,
        successful_sustain_runs,
        total_duration_proven_sustain_runs,
        min_sustain_runs: args.min_sustain_runs,
        remaining_sustain_runs,
        min_minutes_to_sustain_target,
        min_sustain_duration_minutes: args.min_sustain_duration_minutes,
        short_or_unproven_sustain_runs,
        clean_merged_prs,
        min_clean_merged_prs: args.min_clean_merged_prs,
        exceeded,
        missing_evidence,
    })
}

fn is_successful_runner_sustain(run: &WorkflowRunEntry) -> bool {
    run.name == "Runner Sustain"
        && run.status.eq_ignore_ascii_case("completed")
        && run.conclusion.eq_ignore_ascii_case("success")
}

fn runner_sustain_duration_minutes(
    run: &WorkflowRunEntry,
    min_duration_minutes: u64,
) -> Option<u64> {
    if !is_successful_runner_sustain(run) {
        return None;
    }
    let created = run
        .created_at
        .as_deref()
        .and_then(parse_rfc3339_utc_seconds)?;
    let updated = run
        .updated_at
        .as_deref()
        .and_then(parse_rfc3339_utc_seconds)?;
    if updated < created {
        return None;
    }
    let duration_minutes = ((updated - created) / 60) as u64;
    (duration_minutes >= min_duration_minutes).then_some(duration_minutes)
}

fn max_local_runner_idle_gap_minutes(
    runs: &[WorkflowRunEntry],
    window_start: i64,
    window_end: i64,
    min_duration_minutes: u64,
) -> u64 {
    if window_end <= window_start {
        return 0;
    }
    let mut intervals = runs
        .iter()
        .filter_map(|run| local_runner_productive_interval(run, min_duration_minutes))
        .filter_map(|(start, end)| {
            let clipped_start = start.max(window_start);
            let clipped_end = end.min(window_end);
            (clipped_end >= clipped_start).then_some((clipped_start, clipped_end))
        })
        .collect::<Vec<_>>();
    intervals.sort_by_key(|(start, end)| (*start, *end));

    let mut cursor = window_start;
    let mut max_gap_seconds = 0_i64;
    for (start, end) in intervals {
        if start > cursor {
            max_gap_seconds = max_gap_seconds.max(start - cursor);
        }
        if end > cursor {
            cursor = end;
        }
    }
    if window_end > cursor {
        max_gap_seconds = max_gap_seconds.max(window_end - cursor);
    }
    (max_gap_seconds.max(0) as u64).div_ceil(60)
}

fn runner_sustain_interval(
    run: &WorkflowRunEntry,
    min_duration_minutes: u64,
) -> Option<(i64, i64)> {
    if run.name != "Runner Sustain" {
        return None;
    }
    let start = run
        .created_at
        .as_deref()
        .and_then(parse_rfc3339_utc_seconds)?;
    let end = run
        .updated_at
        .as_deref()
        .and_then(parse_rfc3339_utc_seconds)
        .unwrap_or(start);

    if run.status.eq_ignore_ascii_case("queued") || run.status.eq_ignore_ascii_case("in_progress") {
        return Some((start, end.max(start)));
    }
    if runner_sustain_duration_minutes(run, min_duration_minutes).is_some() {
        return Some((start, end.max(start)));
    }
    None
}

fn local_runner_productive_interval(
    run: &WorkflowRunEntry,
    min_sustain_duration_minutes: u64,
) -> Option<(i64, i64)> {
    if run.name == "Runner Sustain" {
        return runner_sustain_interval(run, min_sustain_duration_minutes);
    }
    if !matches!(run.name.as_str(), "CI" | "Semantic PR Title") {
        return None;
    }
    let start = run
        .created_at
        .as_deref()
        .and_then(parse_rfc3339_utc_seconds)?;
    let end = run
        .updated_at
        .as_deref()
        .and_then(parse_rfc3339_utc_seconds)
        .unwrap_or(start);
    Some((start, end.max(start)))
}

fn is_ops_workflow(name: &str) -> bool {
    matches!(name, "Runner Sustain" | "CI" | "Semantic PR Title")
        || is_runner_black_factor_watch_name(name)
}

fn is_runner_black_factor_watch_name(name: &str) -> bool {
    name == "Runner Black Factor Watch" || name.starts_with("Runner Black Factor Watch (")
}

fn is_successful_runner_watch_rehydration(run: &WorkflowRunEntry) -> bool {
    if !is_runner_black_factor_watch_name(&run.name)
        || !run.status.eq_ignore_ascii_case("completed")
        || !run.conclusion.eq_ignore_ascii_case("success")
    {
        return false;
    }
    if run.event.eq_ignore_ascii_case("workflow_run") {
        return true;
    }
    run.event.eq_ignore_ascii_case("workflow_dispatch")
        && run.display_title.contains("sustain_completion")
}

fn is_failed_conclusion(conclusion: &str) -> bool {
    matches!(
        conclusion.to_ascii_lowercase().as_str(),
        "failure" | "timed_out" | "cancelled" | "action_required"
    )
}

fn is_failed_ops_run(run: &WorkflowRunEntry, runs: &[WorkflowRunEntry]) -> bool {
    if !is_failed_conclusion(&run.conclusion) {
        return false;
    }
    if run.conclusion.eq_ignore_ascii_case("cancelled")
        && has_nearby_successful_replacement(run, runs)
    {
        return false;
    }
    true
}

fn has_nearby_successful_replacement(run: &WorkflowRunEntry, runs: &[WorkflowRunEntry]) -> bool {
    let Some(cancelled_at) = run
        .created_at
        .as_deref()
        .and_then(parse_rfc3339_utc_seconds)
    else {
        return false;
    };
    runs.iter().any(|candidate| {
        candidate.name == run.name
            && candidate.head_branch == run.head_branch
            && candidate.status.eq_ignore_ascii_case("completed")
            && candidate.conclusion.eq_ignore_ascii_case("success")
            && candidate
                .created_at
                .as_deref()
                .and_then(parse_rfc3339_utc_seconds)
                .is_some_and(|created| (created - cancelled_at).abs() <= 10 * 60)
    })
}

fn parse_rfc3339_utc_seconds(value: &str) -> Option<i64> {
    // Supports the GitHub API shape used by `gh run list`: YYYY-MM-DDTHH:MM:SSZ.
    if value.len() < 20 || !value.ends_with('Z') {
        return None;
    }
    let year = value.get(0..4)?.parse::<i32>().ok()?;
    let month = value.get(5..7)?.parse::<u32>().ok()?;
    let day = value.get(8..10)?.parse::<u32>().ok()?;
    let hour = value.get(11..13)?.parse::<u32>().ok()?;
    let minute = value.get(14..16)?.parse::<u32>().ok()?;
    let second = value.get(17..19)?.parse::<u32>().ok()?;
    if value.as_bytes().get(4) != Some(&b'-')
        || value.as_bytes().get(7) != Some(&b'-')
        || value.as_bytes().get(10) != Some(&b'T')
        || value.as_bytes().get(13) != Some(&b':')
        || value.as_bytes().get(16) != Some(&b':')
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    // Howard Hinnant's days-from-civil algorithm, returning days since Unix epoch.
    let y = year - i32::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month as i32 + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    i64::from(era * 146_097 + doe - 719_468)
}

fn runner_flow_audit_report(args: &RunnerFlowAuditArgs) -> Result<RunnerFlowAuditReport> {
    let sustain_workflow_present = args
        .root
        .join(".github/workflows/runner-sustain.yml")
        .exists()
        && fs::read_to_string(args.root.join(".github/workflows/runner-sustain.yml"))
            .map(|text| {
                (text.contains("*/10 * * * *") || text.contains("*/5 * * * *"))
                    && text.contains("self-hosted")
                    && text.contains("components-audit --strict")
                    && text.contains("target-mining-audit --strict")
            })
            .unwrap_or(false);

    let runs = if let Some(path) = &args.runs_json {
        parse_json_vec::<WorkflowRunEntry>(path)?
    } else {
        Vec::new()
    };
    let active_runs = runs
        .iter()
        .filter(|run| run.status.eq_ignore_ascii_case("in_progress"))
        .count();
    let queued_runs = runs
        .iter()
        .filter(|run| {
            run.status.eq_ignore_ascii_case("queued") || run.status.eq_ignore_ascii_case("pending")
        })
        .count();

    let prs = if let Some(path) = &args.prs_json {
        parse_json_vec::<PrFlowEntry>(path)?
    } else {
        Vec::new()
    };
    let open_prs = prs.len();
    let mut queued_required_checks = 0;
    let mut failed_required_checks = 0;
    for pr in &prs {
        let runner_health = classify_runner_health(&pr.status_check_rollup);
        queued_required_checks += runner_health.pending_local_checks.len();
        failed_required_checks += runner_health.failed_local_checks.len();
    }

    let idle_without_work = active_runs == 0 && queued_runs == 0 && open_prs == 0;
    let pr_flow_seamless =
        open_prs == 0 || (queued_required_checks == 0 && failed_required_checks == 0);

    let mut missing_evidence = Vec::new();
    if !sustain_workflow_present {
        missing_evidence.push("runner_sustain_workflow");
    }
    if idle_without_work {
        missing_evidence.push("active_or_queued_runner_work");
    }
    if !pr_flow_seamless {
        missing_evidence.push("seamless_pr_flow");
    }

    Ok(RunnerFlowAuditReport {
        kclaw0_target:
            "24/7 dark-factory operation with swarm-scale persistence and green-gated PR flow",
        sustain_workflow_present,
        active_runs,
        queued_runs,
        open_prs,
        queued_required_checks,
        failed_required_checks,
        idle_without_work,
        pr_flow_seamless,
        missing_evidence,
    })
}

fn parse_json_vec<T>(path: &Path) -> Result<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

fn runner_health_report(path: &Path) -> Result<RunnerHealthReport> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("read check rollup {}", path.display()))?;
    let payload = parse_check_rollup(&text)
        .with_context(|| format!("parse check rollup {}", path.display()))?;
    Ok(classify_runner_health(&payload.status_check_rollup))
}

fn parse_check_rollup(text: &str) -> Result<CheckRollupPayload> {
    if let Ok(payload) = serde_json::from_str::<CheckRollupPayload>(text) {
        return Ok(payload);
    }
    let status_check_rollup = serde_json::from_str::<Vec<CheckRollupEntry>>(text)
        .context("parse statusCheckRollup array")?;
    Ok(CheckRollupPayload {
        status_check_rollup,
    })
}

fn classify_runner_health(checks: &[CheckRollupEntry]) -> RunnerHealthReport {
    let mut local_check_states: BTreeMap<String, CheckState> = BTreeMap::new();

    for check in checks
        .iter()
        .filter(|check| is_local_runner_check(&check.name))
    {
        local_check_states
            .entry(check.name.clone())
            .and_modify(|state| *state = state.merged_with(check_state(check)))
            .or_insert_with(|| check_state(check));
    }

    let mut pending_local_checks = Vec::new();
    let mut passed_local_checks = Vec::new();
    let mut failed_local_checks = Vec::new();
    let mut missing_local_checks = Vec::new();

    for required in REQUIRED_LOCAL_CHECKS {
        if !local_check_states.contains_key(*required) {
            missing_local_checks.push((*required).to_string());
        }
    }

    for (name, state) in local_check_states {
        match state {
            CheckState::Pending => pending_local_checks.push(name),
            CheckState::Passed => passed_local_checks.push(name),
            CheckState::Failed => failed_local_checks.push(name),
        }
    }

    let runner_pressure = !pending_local_checks.is_empty();
    let recommendation = if runner_pressure {
        "inspect self-hosted runner services and queued external jobs before waiting indefinitely"
    } else if !missing_local_checks.is_empty() {
        "verify required local checks were scheduled before trusting branch-protection state"
    } else {
        "local self-hosted required checks are not currently queued"
    };
    RunnerHealthReport {
        required_local_checks: REQUIRED_LOCAL_CHECKS
            .iter()
            .map(|check| (*check).to_string())
            .collect(),
        pending_local_checks,
        passed_local_checks,
        failed_local_checks,
        missing_local_checks,
        runner_pressure,
        recommendation,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckState {
    Pending,
    Passed,
    Failed,
}

impl CheckState {
    fn merged_with(self, other: CheckState) -> CheckState {
        match (self, other) {
            (CheckState::Pending, _) | (_, CheckState::Pending) => CheckState::Pending,
            (CheckState::Passed, _) | (_, CheckState::Passed) => CheckState::Passed,
            (CheckState::Failed, CheckState::Failed) => CheckState::Failed,
        }
    }
}

fn check_state(check: &CheckRollupEntry) -> CheckState {
    let status = check.status.to_ascii_lowercase();
    let conclusion = check.conclusion.to_ascii_lowercase();
    if matches!(status.as_str(), "queued" | "pending" | "in_progress") || conclusion.is_empty() {
        CheckState::Pending
    } else if conclusion == "success" {
        CheckState::Passed
    } else {
        CheckState::Failed
    }
}

fn is_local_runner_check(name: &str) -> bool {
    REQUIRED_LOCAL_CHECKS.contains(&name)
}

fn components_audit(args: ComponentsAuditArgs) -> Result<()> {
    let report = components_audit_report(&args.root);
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("fxrun forge-loop components audit");
        println!("  checked components : {}", report.checked_components);
        if report.missing_components.is_empty() {
            println!("  missing components : none");
        } else {
            println!("  missing components :");
            for component in &report.missing_components {
                println!("    - {component}");
            }
        }
    }

    if args.strict && !report.missing_components.is_empty() {
        Err(anyhow!(
            "forge-loop components missing: {}",
            report.missing_components.join(", ")
        ))
    } else {
        Ok(())
    }
}

fn target_mining_audit(args: TargetMiningAuditArgs) -> Result<()> {
    let report = target_mining_audit_report(&args.root);
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("fxrun forge-loop target mining audit");
        println!("  checked targets : {}", report.checked_targets);
        if report.missing_targets.is_empty() {
            println!("  missing targets : none");
        } else {
            println!("  missing targets :");
            for target in &report.missing_targets {
                println!("    - {target}");
            }
        }
    }

    if args.strict && !report.missing_targets.is_empty() {
        Err(anyhow!(
            "forge-loop target mining incomplete: {}",
            report.missing_targets.join(", ")
        ))
    } else {
        Ok(())
    }
}

fn target_mining_audit_report(root: &Path) -> TargetMiningAuditReport {
    let targets = expected_target_mining_targets()
        .into_iter()
        .map(|target| {
            let source_evidence = all_terms_present(
                root,
                &[
                    "docs/forge-loop/codex-target-mining.md",
                    "docs/forge-loop/codex-target-exhaustion-matrix.md",
                    ".agents/skills/forge-loop-research/SKILL.md",
                ],
                target.source_terms,
            );
            let application_evidence = all_file_terms_present(root, target.application_terms);
            let guard_evidence = all_file_terms_present(root, target.guard_terms);
            let mut missing = Vec::new();
            if !source_evidence {
                missing.push("source_evidence");
            }
            if !application_evidence {
                missing.push("application_evidence");
            }
            if !guard_evidence {
                missing.push("guard_evidence");
            }
            TargetMiningStatus {
                id: target.id,
                url: target.url,
                source_evidence,
                application_evidence,
                guard_evidence,
                missing,
            }
        })
        .collect::<Vec<_>>();
    let covered_targets = targets
        .iter()
        .filter(|target| target.missing.is_empty())
        .map(|target| target.id.to_string())
        .collect::<Vec<_>>();
    let missing_targets = targets
        .iter()
        .filter(|target| !target.missing.is_empty())
        .map(|target| target.id.to_string())
        .collect::<Vec<_>>();

    TargetMiningAuditReport {
        checked_targets: targets.len(),
        covered_targets,
        missing_targets,
        targets,
    }
}

fn all_terms_present(root: &Path, files: &[&str], terms: &[&str]) -> bool {
    let text = files
        .iter()
        .filter_map(|path| fs::read_to_string(root.join(path)).ok())
        .collect::<Vec<_>>()
        .join("\n");
    terms.iter().all(|term| text.contains(term))
}

fn all_file_terms_present(root: &Path, terms: &[(&str, &str)]) -> bool {
    terms.iter().all(|(path, term)| {
        fs::read_to_string(root.join(path))
            .map(|text| text.contains(term))
            .unwrap_or(false)
    })
}

fn expected_target_mining_targets() -> Vec<TargetMiningTarget> {
    vec![
        TargetMiningTarget {
            id: "codex-github-action",
            url: "https://developers.openai.com/codex/github-action",
            source_terms: &[
                "developers.openai.com/codex/github-action",
                "final-message",
                "--output-schema",
            ],
            application_terms: &[
                (
                    ".github/workflows/codex-forge-loop.yml",
                    "openai/codex-action@v1",
                ),
                (".github/workflows/codex-forge-loop.yml", "codex-args:"),
                (".github/workflows/codex-forge-loop.yml", "--output-schema"),
                (
                    ".github/codex/schemas/forge-loop-output.schema.json",
                    "component_inventory",
                ),
                (
                    ".github/codex/schemas/forge-loop-output.schema.json",
                    "auto_compact_continuity",
                ),
                (
                    ".github/workflows/codex-forge-loop.yml",
                    "features.auto_compaction=true",
                ),
            ],
            guard_terms: &[
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "codex_github_action_workflow_uses_documented_controls",
                ),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "target_mining_audit_report",
                ),
            ],
        },
        TargetMiningTarget {
            id: "codex-permissions",
            url: "https://developers.openai.com/codex/permissions",
            source_terms: &[
                "developers.openai.com/codex/permissions",
                "default_permissions",
                "sandbox_mode",
            ],
            application_terms: &[
                (
                    ".codex/permissions/forge-loop-workspace.toml",
                    "default_permissions",
                ),
                (".codex/permissions/forge-loop-workspace.toml", "**/*.env"),
                (
                    ".codex/hooks/forge_loop_permission_request.py",
                    "profile_is_blueprint_only",
                ),
            ],
            guard_terms: &[
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "codex_deep_target_mining_surfaces_are_guarded",
                ),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "do not mix active permission profiles with sandbox_mode",
                ),
            ],
        },
        TargetMiningTarget {
            id: "codex-subagents",
            url: "https://developers.openai.com/codex/subagents",
            source_terms: &[
                "developers.openai.com/codex/subagents",
                "nickname_candidates",
                "SubagentStart",
            ],
            application_terms: &[
                (
                    ".codex/agents/forge-loop-researcher.toml",
                    "nickname_candidates",
                ),
                (
                    ".codex/agents/forge-loop-ci-sentinel.toml",
                    "nickname_candidates",
                ),
                (".codex/hooks.json", "SubagentStart"),
                (".codex/hooks.json", "SubagentStop"),
            ],
            guard_terms: &[
                ("crates/runner-cli/src/forge_loop.rs", "subagent-roster"),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "ci-sentinel-subagent",
                ),
            ],
        },
        TargetMiningTarget {
            id: "awesome-codex-cli",
            url: "https://github.com/RoggeOhta/awesome-codex-cli",
            source_terms: &["RoggeOhta/awesome-codex-cli", "Monitoring", "MCP", "CI/CD"],
            application_terms: &[
                (
                    ".agents/skills/forge-loop-research/SKILL.md",
                    "RoggeOhta/awesome-codex-cli",
                ),
                (
                    "docs/forge-loop/codex-target-exhaustion-matrix.md",
                    "workflow/session managers",
                ),
            ],
            guard_terms: &[
                ("crates/runner-cli/src/forge_loop.rs", "target-mining-audit"),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "target-mining-ledger",
                ),
            ],
        },
        TargetMiningTarget {
            id: "oh-my-codex",
            url: "https://github.com/Yeachan-Heo/oh-my-codex",
            source_terms: &[
                "Yeachan-Heo/oh-my-codex",
                "named worktree",
                "doctor",
                "native hook",
            ],
            application_terms: &[
                (".codex/prompts/forge-loop.md", "isolated named worktrees"),
                (
                    ".codex/hooks/forge_loop_compact_summary.py",
                    "covered_targets",
                ),
                (".codex/config.toml", "auto_compaction = true"),
                (".codex/prompts/compact-forge-loop.md", "next action"),
                (
                    "docs/forge-loop/codex-target-exhaustion-matrix.md",
                    "deep-interview",
                ),
            ],
            guard_terms: &[
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "target_mining_audit_report",
                ),
                ("crates/runner-cli/src/forge_loop.rs", "oh-my-codex"),
            ],
        },
        TargetMiningTarget {
            id: "kclaw0",
            url: "https://github.com/drdave-flexnetos/kclaw0",
            source_terms: &[
                "drdave-flexnetos/kclaw0",
                "24/7 autonomous operation",
                "self-upgrade pipeline",
                "GitHub label state machine",
                "holdout validation",
            ],
            application_terms: &[
                (
                    "docs/forge-loop/kclaw0-runner-flow-target.md",
                    "runner-black-factor-audit --strict",
                ),
                (
                    "docs/forge-loop/kclaw0-runner-flow-target.md",
                    "runner-ops-slo-audit --strict",
                ),
                (
                    "docs/forge-loop/kclaw0-runner-flow-target.md",
                    "runner-fleet-audit --strict",
                ),
                (
                    "docs/forge-loop/agentic-system-proof.md",
                    "agentic-system-audit --strict",
                ),
                (
                    ".github/workflows/runner-sustain.yml",
                    "name: Runner Sustain",
                ),
                (
                    ".github/workflows/runner-black-factor-watch.yml",
                    "name: Runner Black Factor Watch",
                ),
                (
                    ".github/workflows/agentic-system-watch.yml",
                    "name: Agentic System Watch",
                ),
            ],
            guard_terms: &[
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "runner_black_factor_audit_accepts_kclaw0_window_fixture",
                ),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "runner_ops_slo_audit_accepts_event_rehydrated_burn_in",
                ),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "runner_fleet_audit_flags_external_repo_lane_pressure",
                ),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "agentic_system_audit_report",
                ),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "agentic_system_watch_dispatches_codex_growth_safely",
                ),
                ("crates/runner-cli/src/forge_loop.rs", "kclaw0"),
            ],
        },
        TargetMiningTarget {
            id: "kclaw0-referenced-resources",
            url: "https://github.com/drdave-flexnetos/kclaw0",
            source_terms: &[
                "kclaw0 referenced resources",
                "strongdm/attractor",
                "coleam00/Archon",
                "Conway-Research/automaton",
                "oh-my-pi",
            ],
            application_terms: &[
                ("docs/kclaw0-upgrade-ledger.md", "Prior-art batch"),
                (
                    "docs/kclaw0-upgrade-ledger.md",
                    "Cycle-11 deep-research sweep",
                ),
                (
                    "docs/kclaw0-upgrade-ledger.md",
                    "Cycle-16 deep-research sweep",
                ),
                ("docs/kclaw0-upgrade-ledger.md", "strongdm/attractor"),
                ("docs/kclaw0-upgrade-ledger.md", "coleam00/Archon"),
                ("docs/kclaw0-upgrade-ledger.md", "Conway-Research/automaton"),
            ],
            guard_terms: &[
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "target_mining_audit_report",
                ),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "kclaw0-referenced-resources",
                ),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "kclaw0 referenced resources must be first-class target-mining coverage",
                ),
            ],
        },
    ]
}

fn components_audit_report(root: &Path) -> ComponentsAuditReport {
    let components = expected_loop_components()
        .into_iter()
        .map(|component| {
            let present = root.join(component.path).exists();
            LoopComponentStatus {
                id: component.id,
                surface: component.surface,
                path: component.path,
                present,
                rationale: component.rationale,
            }
        })
        .collect::<Vec<_>>();
    let present_components = components
        .iter()
        .filter(|component| component.present)
        .map(|component| component.id.to_string())
        .collect::<Vec<_>>();
    let missing_components = components
        .iter()
        .filter(|component| !component.present)
        .map(|component| component.id.to_string())
        .collect::<Vec<_>>();

    ComponentsAuditReport {
        checked_components: components.len(),
        present_components,
        missing_components,
        components,
    }
}

fn expected_loop_components() -> Vec<LoopComponent> {
    vec![
        LoopComponent {
            id: "codex-prompt",
            surface: "prompt",
            path: ".codex/prompts/forge-loop.md",
            rationale: "Codex GitHub Action docs recommend prompt-file inputs stored under .github/codex/prompts; this repo also keeps the local forge-loop prompt as a Codex prompt artifact.",
        },
        LoopComponent {
            id: "compact-prompt",
            surface: "prompt",
            path: ".codex/prompts/compact-forge-loop.md",
            rationale: "Long-running forge-loop sessions need an explicit compact prompt so auto-compaction preserves phase, source coverage, validation state, and next action instead of context rot.",
        },
        LoopComponent {
            id: "project-config",
            surface: "config",
            path: ".codex/config.toml",
            rationale: "Advanced Codex config supports trusted project-scoped .codex/config.toml layers for repo-local model, sandbox, agent, MCP, and skill defaults.",
        },
        LoopComponent {
            id: "hooks",
            surface: "hooks",
            path: ".codex/hooks.json",
            rationale: "Advanced Codex config supports repo-local hooks.json for lifecycle hooks next to an active project config layer.",
        },
        LoopComponent {
            id: "permission-request-hook",
            surface: "hooks",
            path: ".codex/hooks/forge_loop_permission_request.py",
            rationale: "Codex PermissionRequest hooks can witness approval posture and ensure the permission-profile blueprint stays separate from active sandbox settings.",
        },
        LoopComponent {
            id: "post-tool-hook",
            surface: "hooks",
            path: ".codex/hooks/forge_loop_post_tool_use.py",
            rationale: "Codex PostToolUse hooks let the harness re-check critical loop surfaces after mutating tool calls.",
        },
        LoopComponent {
            id: "compact-summary-hook",
            surface: "hooks",
            path: ".codex/hooks/forge_loop_compact_summary.py",
            rationale: "Codex PreCompact/PostCompact hooks preserve target-mining continuity across context compaction.",
        },
        LoopComponent {
            id: "hook-manifest",
            surface: "hooks",
            path: ".codex/hooks/forge-loop-hooks.manifest.json",
            rationale: "A machine-readable hook manifest keeps lifecycle event coverage, script paths, and expected JSON keys auditable.",
        },
        LoopComponent {
            id: "rules",
            surface: "rules",
            path: ".codex/rules/forge-loop.rules",
            rationale: "Codex rules provide executable command-policy guardrails with inline match/not_match examples.",
        },
        LoopComponent {
            id: "subagent",
            surface: "agents",
            path: ".codex/agents/forge-loop-auditor.toml",
            rationale: "Codex custom agents live under .codex/agents and can encode narrow reviewer or auditor roles with model and sandbox defaults.",
        },
        LoopComponent {
            id: "subagent-roster",
            surface: "agents",
            path: ".codex/agents/forge-loop-researcher.toml",
            rationale: "Codex subagent docs recommend narrow project-scoped custom agents for parallelizable research and review work.",
        },
        LoopComponent {
            id: "ci-sentinel-subagent",
            surface: "agents",
            path: ".codex/agents/forge-loop-ci-sentinel.toml",
            rationale: "Programmatic Codex Action runs need a focused CI/release readiness reviewer for workflow, artifact, and gate evidence.",
        },
        LoopComponent {
            id: "permission-profile-blueprint",
            surface: "permissions",
            path: ".codex/permissions/forge-loop-workspace.toml",
            rationale: "Codex permission profiles provide a least-privilege migration target, but must stay separate while sandbox_mode remains active.",
        },
        LoopComponent {
            id: "skill",
            surface: "skills",
            path: ".agents/skills/forge-loop-research/SKILL.md",
            rationale: "The forge-loop research skill is the existing reusable workflow for strict-upgrade self-improvement research.",
        },
        LoopComponent {
            id: "github-action",
            surface: "tools",
            path: ".github/workflows/ci.yml",
            rationale: "GitHub workflow configuration is the current CI/tool gate surface for required forge-loop checks.",
        },
        LoopComponent {
            id: "runner-sustain-workflow",
            surface: "tools",
            path: ".github/workflows/runner-sustain.yml",
            rationale: "Runner sustain automation keeps self-hosted runner slots doing useful forge-loop audits on a schedule and by manual dispatch.",
        },
        LoopComponent {
            id: "runner-black-factor-watch-workflow",
            surface: "tools",
            path: ".github/workflows/runner-black-factor-watch.yml",
            rationale: "The black-factor watch workflow audits runner flow, refills Runner Sustain when sustain work disappears, and uploads run/PR evidence artifacts for the 12-hour proof window.",
        },
        LoopComponent {
            id: "agentic-system-watch-workflow",
            surface: "tools",
            path: ".github/workflows/agentic-system-watch.yml",
            rationale: "The agentic system watch proves the composite 24/7 audit and safely dispatches Codex Forge Loop self-improvement when credentials and PR pressure allow.",
        },
        LoopComponent {
            id: "codex-github-action",
            surface: "tools",
            path: ".github/workflows/codex-forge-loop.yml",
            rationale: "Codex GitHub Action docs describe openai/codex-action with prompt-file, codex-args, model, effort, sandbox, output-file, and safety controls for programmatic loop runs.",
        },
        LoopComponent {
            id: "codex-output-schema",
            surface: "tools",
            path: ".github/codex/schemas/forge-loop-output.schema.json",
            rationale: "Codex GitHub Action docs allow --output-schema through codex-args so forge-loop automation can require structured evidence.",
        },
        LoopComponent {
            id: "codex-continuity-schema",
            surface: "tools",
            path: ".github/codex/schemas/forge-loop-output.schema.json",
            rationale: "The structured output schema must require auto-compaction continuity evidence for every action-driven loop run.",
        },
        LoopComponent {
            id: "target-mining-ledger",
            surface: "docs",
            path: "docs/forge-loop/codex-target-mining.md",
            rationale: "Deep target mining needs a source-attributed extraction ledger so future loops can distinguish applied upgrades from unmined leads.",
        },
        LoopComponent {
            id: "target-exhaustion-matrix",
            surface: "docs",
            path: "docs/forge-loop/codex-target-exhaustion-matrix.md",
            rationale: "The target exhaustion matrix maps each required source to extracted categories, applied surfaces, and regression guards.",
        },
        LoopComponent {
            id: "kclaw0-runner-flow-target",
            surface: "docs",
            path: "docs/forge-loop/kclaw0-runner-flow-target.md",
            rationale: "The kclaw0 runner-flow target records the dark-factory/swarm evidence requirements before the harness can claim runners exceeded the target.",
        },
        LoopComponent {
            id: "agentic-system-proof",
            surface: "docs",
            path: "docs/forge-loop/agentic-system-proof.md",
            rationale: "The end-to-end agentic-system proof maps always-researching, evaluating, adapting, and growing claims to a single strict audit gate.",
        },
        LoopComponent {
            id: "worktree-isolation-contract",
            surface: "worktrees",
            path: ".codex/worktrees/forge-loop-isolation.toml",
            rationale: "Forge-loop cycles need an explicit worktree isolation contract so concurrent mutating runs cannot trample one another.",
        },
        LoopComponent {
            id: "cycle-evidence-checklist",
            surface: "checklists",
            path: ".codex/checklists/forge-loop-cycle.toml",
            rationale: "Every forge-loop cycle needs a durable evidence checklist for tests, audits, PR state, merge evidence, and main fast-forward proof.",
        },
        LoopComponent {
            id: "deep-research-exhaustion-report",
            surface: "docs",
            path: "docs/forge-loop/deep-research-exhaustion-2026-06-27.md",
            rationale: "The deep-research exhaustion report records the mined target categories and the applied auto-compaction continuity contract.",
        },
    ]
}

fn docs_drift(args: DocsDriftArgs) -> Result<()> {
    let report = docs_drift_report(&args.root)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if report.drift.is_empty() {
        println!(
            "forge-loop docs drift guard passed: {} applied features checked",
            report.checked_features
        );
    } else {
        println!("forge-loop docs drift guard failed:");
        for item in &report.drift {
            println!("  - {item}");
        }
    }

    if report.drift.is_empty() {
        Ok(())
    } else {
        Err(anyhow!("forge-loop docs drift detected"))
    }
}

pub fn docs_drift_report(root: &Path) -> Result<DocsDriftReport> {
    let ledger_path = root.join("docs/kclaw0-upgrade-ledger.md");
    let ledger = fs::read_to_string(&ledger_path)
        .with_context(|| format!("read docs ledger {}", ledger_path.display()))?;

    let mut drift = Vec::new();
    let features = applied_doc_features();
    for feature in &features {
        let module_path = root.join(feature.module_path);
        if !module_path.exists() {
            continue;
        }

        let matching_blocks = markdown_blocks_containing(&ledger, feature.title);
        if matching_blocks.is_empty() {
            drift.push(format!(
                "{} exists at {} but is missing from docs/kclaw0-upgrade-ledger.md",
                feature.title, feature.module_path
            ));
            continue;
        }

        for block in matching_blocks {
            if block_is_queued(&block) {
                drift.push(format!(
                    "{} exists at {} but is still documented as queued/backlog work",
                    feature.title, feature.module_path
                ));
            }
        }
    }

    Ok(DocsDriftReport {
        checked_features: features.len(),
        drift,
    })
}

pub fn codex_program() -> String {
    std::env::var("FXRUN_CODEX")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| {
            if Path::new(DEFAULT_CODEX).exists() {
                DEFAULT_CODEX.into()
            } else {
                "codex".into()
            }
        })
}

pub fn codex_invocation(prompt: String) -> CodexInvocation {
    CodexInvocation {
        program: codex_program(),
        args: vec![
            "exec".into(),
            "--json".into(),
            "--sandbox".into(),
            "workspace-write".into(),
            "--ignore-user-config".into(),
            "--config".into(),
            "approval_policy=\"never\"".into(),
            "--config".into(),
            "features.auto_compaction=true".into(),
            "--config".into(),
            format!("model_auto_compact_token_limit={AUTO_COMPACT_TOKEN_LIMIT}"),
            "--config".into(),
            "model_auto_compact_token_limit_scope=\"total\"".into(),
            "--config".into(),
            format!("tool_output_token_limit={TOOL_OUTPUT_TOKEN_LIMIT}"),
            "--config".into(),
            format!("experimental_compact_prompt_file=\"{COMPACT_PROMPT_PATH}\""),
            prompt,
        ],
    }
}

pub fn research_sources() -> Vec<ResearchSource> {
    vec![
        ResearchSource { id: "openai-codex", url: "https://github.com/openai/codex", purpose: "Codex Rust CLI behavior, noninteractive execution, JSONL, and upstream issues" },
        ResearchSource { id: "codex-github-action-docs", url: "https://developers.openai.com/codex/github-action", purpose: "Codex Action prompt-file, codex-args, sandbox, safety-strategy, output, and structured schema controls" },
        ResearchSource { id: "codex-permissions-docs", url: "https://developers.openai.com/codex/permissions", purpose: "Permission-profile migration, filesystem/network least privilege, and sandbox/profile non-composition rules" },
        ResearchSource { id: "codex-subagents-docs", url: "https://developers.openai.com/codex/subagents", purpose: "Project custom agents, explicit fan-out, inherited sandbox behavior, and max thread/depth controls" },
        ResearchSource { id: "awesome-codex-cli", url: "https://github.com/RoggeOhta/awesome-codex-cli", purpose: "Codex ecosystem tools, skills, plugins, MCP servers, and orchestration patterns" },
        ResearchSource { id: "oh-my-codex", url: "https://github.com/Yeachan-Heo/oh-my-codex", purpose: "multi-agent teams, hooks, HUDs, and Codex orchestration UX" },
        ResearchSource { id: "crates-io", url: "https://crates.io", purpose: "Rust crates that improve loop reliability, accuracy, speed, tracing, and scheduling" },
        ResearchSource { id: "kclaw0", url: "https://github.com/drdave-flexnetos/kclaw0", purpose: "local dark-factory/self-upgrade prior art and governance patterns" },
    ]
}

struct AppliedDocFeature {
    title: &'static str,
    module_path: &'static str,
}

fn applied_doc_features() -> Vec<AppliedDocFeature> {
    vec![
        AppliedDocFeature {
            title: "State-gated route admission",
            module_path: "crates/runner-core/src/stategate.rs",
        },
        AppliedDocFeature {
            title: "Deterministic route-selection contract",
            module_path: "crates/runner-core/src/router.rs",
        },
        AppliedDocFeature {
            title: "Idle / liveness watchdog",
            module_path: "crates/runner-core/src/liveness.rs",
        },
        AppliedDocFeature {
            title: "Delegation-target allowlist",
            module_path: "crates/runner-core/src/targets.rs",
        },
        AppliedDocFeature {
            title: "Per-target single-flight mutex",
            module_path: "crates/runner-core/src/singleflight.rs",
        },
        AppliedDocFeature {
            title: "Rule-citation audit schema",
            module_path: "crates/runner-core/src/events.rs",
        },
    ]
}

fn markdown_blocks_containing(text: &str, needle: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = Vec::new();
    let mut context = Vec::new();

    for line in text.lines() {
        if line.starts_with("## ") || line.starts_with("### ") {
            if !current.is_empty() {
                blocks.push(current.join("\n"));
                current.clear();
            }
            context.retain(|heading: &&str| heading.starts_with("## ") && line.starts_with("### "));
            context.push(line);
        }
        let starts_block = line.starts_with("- ")
            || line
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit() && line.contains(". **"));
        if starts_block && !current.is_empty() {
            blocks.push(current.join("\n"));
            current.clear();
        }
        if starts_block {
            current.extend(context.iter().copied());
        }
        current.push(line);
    }
    if !current.is_empty() {
        blocks.push(current.join("\n"));
    }

    blocks
        .into_iter()
        .filter(|block| block.contains(needle))
        .collect()
}

fn block_is_queued(block: &str) -> bool {
    if block.contains("APPLIED") {
        return false;
    }
    block.contains("**Queued")
        || block.contains("— Queued")
        || block.contains("- ▷")
        || block.contains("queued after")
        || block.contains("still said “Queued”")
        || block.contains("deep code audit backlog")
        || block.contains("Tier 0")
        || block.contains("Tier 1")
        || block.contains("Tier 2")
}

pub fn evaluate(input: EvalInput) -> EvalReport {
    let mut score: i16 = 0;
    let mut reasons = Vec::new();

    if input.red_test_first {
        score += 25;
        reasons.push("red test evidence present");
    }
    if input.gates_passed {
        score += 35;
        reasons.push("repository gates passed");
    }
    if input.retry_count <= 1 {
        score += 10;
        reasons.push("low retry count");
    } else {
        score -= (input.retry_count as i16 - 1) * 5;
    }
    if input.useful_research_items > 0 {
        score += (input.useful_research_items.min(3) as i16) * 5;
        reasons.push("research produced actionable findings");
    }
    if input.runtime_secs <= 900 {
        score += 10;
        reasons.push("runtime within speed budget");
    }
    if (1..=12).contains(&input.diff_files) {
        score += 5;
        reasons.push("diff size is reviewable");
    }

    let score = score.clamp(0, 100) as u8;
    let verdict = if score >= 85 {
        "promote"
    } else if score >= 70 {
        "upgrade-candidate"
    } else if score >= 50 {
        "hold-and-repair"
    } else {
        "quarantine"
    };
    EvalReport {
        score,
        verdict,
        upgrade_allowed: score >= 70
            && input.gates_passed
            && input.red_test_first
            && input.diff_files > 0,
        reasons,
    }
}

fn parse_eval_metrics(path: &Path) -> Result<EvalInput> {
    let text =
        fs::read_to_string(path).with_context(|| format!("read metrics {}", path.display()))?;
    let input: EvalInput =
        serde_json::from_str(&text).with_context(|| format!("parse metrics {}", path.display()))?;
    validate_eval_input(&input).with_context(|| format!("validate metrics {}", path.display()))?;
    Ok(input)
}

fn parse_cycle_manifest(path: &Path) -> Result<CycleManifest> {
    let text =
        fs::read_to_string(path).with_context(|| format!("read manifest {}", path.display()))?;
    let manifest: CycleManifest = serde_json::from_str(&text)
        .with_context(|| format!("parse manifest {}", path.display()))?;
    validate_cycle_manifest(&manifest)
        .with_context(|| format!("validate manifest {}", path.display()))?;
    Ok(manifest)
}

fn validate_cycle_manifest(manifest: &CycleManifest) -> Result<()> {
    if manifest.schema_version != CYCLE_MANIFEST_SCHEMA_VERSION {
        return Err(anyhow!(
            "schema_version {} does not match supported version {}",
            manifest.schema_version,
            CYCLE_MANIFEST_SCHEMA_VERSION
        ));
    }

    let expected_pr_title = cycle_pr_title(&manifest.goal);
    if manifest.pr_title != expected_pr_title {
        return Err(anyhow!(
            "pr_title {:?} does not match expected {:?}",
            manifest.pr_title,
            expected_pr_title
        ));
    }

    let expected_prompt_hash = runner_core::constitution::hash(
        cycle_prompt(&manifest.goal, manifest.auto_merge).as_bytes(),
    );
    if manifest.prompt_sha256 != expected_prompt_hash {
        return Err(anyhow!(
            "prompt_sha256 {:?} does not match expected {:?}",
            manifest.prompt_sha256,
            expected_prompt_hash
        ));
    }

    if !manifest.once {
        return Err(anyhow!("once must be true for isolated forge-loop cycles"));
    }
    if !manifest.strict_upgrade_only {
        return Err(anyhow!(
            "strict_upgrade_only must be true for forge-loop self-upgrades"
        ));
    }
    if manifest.phases != required_phases() {
        return Err(anyhow!(
            "phases do not match the required forge-loop phase order"
        ));
    }
    Ok(())
}

fn default_cycle_manifest_schema_version() -> u8 {
    CYCLE_MANIFEST_SCHEMA_VERSION
}

fn validate_eval_input(input: &EvalInput) -> Result<()> {
    if input.retry_count > MAX_EVAL_RETRY_COUNT {
        return Err(anyhow!(
            "retry_count {} exceeds maximum supported retry count {}",
            input.retry_count,
            MAX_EVAL_RETRY_COUNT
        ));
    }
    Ok(())
}

impl EvalInput {
    pub fn fixture() -> Self {
        Self {
            red_test_first: true,
            gates_passed: true,
            retry_count: 0,
            useful_research_items: 2,
            runtime_secs: 300,
            diff_files: 6,
        }
    }
}

fn cycle_prompt(goal: &str, auto_merge: bool) -> String {
    let pr_title = cycle_pr_title(goal);
    format!(
        "Run a Codex TDD forge-loop cycle for this Rust repo. Goal: {goal}. Do not start another cycle. Keep auto-compaction enabled and preserve phase/source/validation/next-action continuity in compact summaries. Required phases: write/verify a red test first, implement the smallest passing change, run fmt/clippy/tests/audit, evaluate the run, research one reliability/accuracy/speed improvement, and if a self-upgrade is warranted commit, push, open a PR with PR title '{pr_title}', and {}. Strict upgrade only: no downgrades or removals without installed replacement and parity proof.",
        if auto_merge { "auto-merge once green when repository settings allow" } else { "leave the PR ready for review" }
    )
}

fn cycle_pr_title(goal: &str) -> String {
    if let Some(cycle) = cycle_number_from_goal(goal) {
        format!("chore: forge loop cycle {cycle:02}")
    } else {
        "chore: forge loop self-upgrade".into()
    }
}

fn cycle_number_from_goal(goal: &str) -> Option<u8> {
    let lower = goal.to_ascii_lowercase();
    for (cycle_at, _) in lower.match_indices("cycle") {
        let after_cycle = &goal[cycle_at + "cycle".len()..];
        let digits = after_cycle
            .trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '-' || c == '#')
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>();
        if let Ok(cycle) = digits.parse() {
            return Some(cycle);
        }
    }
    None
}

fn cycle_manifest(args: &RunArgs) -> CycleManifest {
    let prompt = cycle_prompt(&args.goal, args.auto_merge);
    CycleManifest {
        schema_version: CYCLE_MANIFEST_SCHEMA_VERSION,
        goal: args.goal.clone(),
        pr_title: cycle_pr_title(&args.goal),
        prompt_sha256: runner_core::constitution::hash(prompt.as_bytes()),
        once: args.once,
        auto_merge: args.auto_merge,
        strict_upgrade_only: true,
        phases: required_phases(),
    }
}

fn required_phases() -> Vec<CyclePhase> {
    vec![
        CyclePhase::Red,
        CyclePhase::Implement,
        CyclePhase::Gate,
        CyclePhase::Evaluate,
        CyclePhase::Research,
        CyclePhase::Upgrade,
    ]
}

fn research_prompt(focus: &str, sources: &[ResearchSource]) -> String {
    let list = sources
        .iter()
        .map(|s| format!("- {}: {} ({})", s.id, s.url, s.purpose))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Research Codex forge-loop improvements focused on {focus}. Scan these references and return actionable, source-attributed upgrades for reliability, accuracy, and speed:\n{list}"
    )
}

fn timestamp_label() -> Result<String> {
    timestamp_label_for(SystemTime::now())
}

fn timestamp_label_for(time: SystemTime) -> Result<String> {
    let elapsed = time
        .duration_since(UNIX_EPOCH)
        .context("system clock before UNIX_EPOCH")?;
    Ok(format!(
        "cycle-{}-{:09}",
        elapsed.as_secs(),
        elapsed.subsec_nanos()
    ))
}

fn append_event(path: &Path, event: CycleEvent<'_>) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open event log {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(&event)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_invocation_forces_auto_compaction_continuity() {
        let inv = codex_invocation("do work".into());
        let joined = inv.args.join("\n");

        assert!(joined.contains("features.auto_compaction=true"));
        assert!(joined.contains("model_auto_compact_token_limit=3000000"));
        assert!(joined.contains("model_auto_compact_token_limit_scope=\"total\""));
        assert!(joined.contains("tool_output_token_limit=12000"));
        assert!(joined
            .contains("experimental_compact_prompt_file=\".codex/prompts/compact-forge-loop.md\""));
    }

    #[test]
    fn codex_invocation_uses_noninteractive_json_workspace_write() {
        let inv = codex_invocation("do work".into());
        assert!(inv.program.ends_with("codex") || inv.program == DEFAULT_CODEX);
        assert_eq!(inv.args[0], "exec");
        assert!(inv.args.contains(&"--json".into()));
        assert!(inv
            .args
            .windows(2)
            .any(|w| w == ["--sandbox", "workspace-write"]));
        assert!(inv.args.contains(&"--ignore-user-config".into()));
        assert!(inv
            .args
            .windows(2)
            .any(|w| w == ["--config", "approval_policy=\"never\""]));
        assert_eq!(inv.args.last().unwrap(), "do work");
    }

    #[test]
    fn research_sources_include_required_refs() {
        let ids = research_sources()
            .into_iter()
            .map(|s| s.id)
            .collect::<Vec<_>>();
        assert!(ids.contains(&"openai-codex"));
        assert!(ids.contains(&"codex-github-action-docs"));
        assert!(ids.contains(&"codex-permissions-docs"));
        assert!(ids.contains(&"codex-subagents-docs"));
        assert!(ids.contains(&"awesome-codex-cli"));
        assert!(ids.contains(&"oh-my-codex"));
        assert!(ids.contains(&"crates-io"));
        assert!(ids.contains(&"kclaw0"));
    }

    #[test]
    fn evaluation_promotes_green_tdd_runs() {
        let report = evaluate(EvalInput::fixture());
        assert!(report.score >= 70);
        assert!(report.upgrade_allowed);
        assert!(matches!(report.verdict, "promote" | "upgrade-candidate"));
    }

    #[test]
    fn evaluation_rejects_no_change_self_upgrades() {
        let report = evaluate(EvalInput {
            diff_files: 0,
            ..EvalInput::fixture()
        });

        assert!(
            !report.upgrade_allowed,
            "strict-upgrade evaluation must not allow a zero-diff self-upgrade"
        );
        assert!(
            !report.reasons.contains(&"diff size is reviewable"),
            "a zero-diff run is not a reviewable upgrade diff"
        );
    }

    #[test]
    fn metrics_parser_rejects_impossible_retry_count() {
        let path = std::env::temp_dir().join(format!(
            "fxrun-eval-metrics-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::write(
            &path,
            r#"{
                "red_test_first": true,
                "gates_passed": true,
                "retry_count": 255,
                "useful_research_items": 1,
                "runtime_secs": 120,
                "diff_files": 1
            }"#,
        )
        .expect("metrics");

        let error = parse_eval_metrics(&path).expect_err("impossible retry count must fail");
        assert!(
            error.root_cause().to_string().contains("retry_count"),
            "error should name the invalid field: {error}"
        );

        fs::remove_file(path).ok();
    }

    #[test]
    fn evaluation_quarantines_missing_red_or_gates() {
        let report = evaluate(EvalInput {
            red_test_first: false,
            gates_passed: false,
            retry_count: 4,
            useful_research_items: 0,
            runtime_secs: 2000,
            diff_files: 50,
        });
        assert!(report.score < 50);
        assert_eq!(report.verdict, "quarantine");
        assert!(!report.upgrade_allowed);
    }

    #[test]
    fn research_prompt_names_sources_and_focus() {
        let prompt = research_prompt("speed", &research_sources());
        assert!(prompt.contains("speed"));
        assert!(prompt.contains("github.com/openai/codex"));
        assert!(prompt.contains("crates.io"));
    }

    #[test]
    fn timestamp_labels_include_subsecond_entropy() {
        let label = timestamp_label_for(
            UNIX_EPOCH + std::time::Duration::from_secs(1) + std::time::Duration::from_millis(1),
        )
        .expect("label");

        assert_eq!(label, "cycle-1-001000000");
    }

    #[test]
    fn cycle_manifest_records_schema_version() {
        let manifest = cycle_manifest(&RunArgs {
            goal: "upgrade manifest schema witness".into(),
            out: PathBuf::from("_work/forge-loop"),
            dry_run: true,
            auto_merge: true,
            once: true,
        });

        assert_eq!(manifest.schema_version, CYCLE_MANIFEST_SCHEMA_VERSION);
    }

    #[test]
    fn cycle_manifest_records_once_strict_phase_contract() {
        let manifest = cycle_manifest(&RunArgs {
            goal: "cycle 05 reliability upgrade".into(),
            out: PathBuf::from("_work/forge-loop"),
            dry_run: true,
            auto_merge: true,
            once: true,
        });

        assert_eq!(manifest.goal, "cycle 05 reliability upgrade");
        assert_eq!(manifest.pr_title, "chore: forge loop cycle 05");
        assert!(manifest.once);
        assert!(manifest.auto_merge);
        assert!(manifest.strict_upgrade_only);
        assert_eq!(
            manifest.phases,
            vec![
                CyclePhase::Red,
                CyclePhase::Implement,
                CyclePhase::Gate,
                CyclePhase::Evaluate,
                CyclePhase::Research,
                CyclePhase::Upgrade,
            ]
        );
    }

    #[test]
    fn run_rejects_multi_cycle_requests_before_planning() {
        let out = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-run-once-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));

        let error = run(RunArgs {
            goal: "attempt a forbidden second cycle".into(),
            out: out.clone(),
            dry_run: true,
            auto_merge: true,
            once: false,
        })
        .expect_err("run must reject multi-cycle requests before writing a plan");

        assert!(
            error.root_cause().to_string().contains("once"),
            "error should name the single-cycle guard: {error}"
        );
        assert!(
            !out.exists(),
            "a rejected multi-cycle request must not leave cycle artifacts"
        );

        fs::remove_dir_all(out).ok();
    }

    #[test]
    fn cycle_manifest_records_deterministic_pr_title() {
        let manifest = cycle_manifest(&RunArgs {
            goal: "Resume the interrupted 10-cycle objective: execute isolated cycle 08 of 10"
                .into(),
            out: PathBuf::from("_work/forge-loop"),
            dry_run: true,
            auto_merge: true,
            once: true,
        });

        assert_eq!(manifest.pr_title, "chore: forge loop cycle 08");
    }

    #[test]
    fn cycle_manifest_records_prompt_hash_witness() {
        let args = RunArgs {
            goal: "Resume the interrupted 10-cycle objective: execute isolated cycle 09 of 10"
                .into(),
            out: PathBuf::from("_work/forge-loop"),
            dry_run: true,
            auto_merge: true,
            once: true,
        };

        let manifest = cycle_manifest(&args);
        let prompt = cycle_prompt(&args.goal, args.auto_merge);

        assert_eq!(
            manifest.prompt_sha256,
            runner_core::constitution::hash(prompt.as_bytes())
        );
    }

    #[test]
    fn eval_manifest_rejects_prompt_hash_mismatch() {
        let path = std::env::temp_dir().join(format!(
            "fxrun-cycle-manifest-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::write(
            &path,
            r#"{
                "goal": "Resume the interrupted 10-cycle objective: execute isolated cycle 10 of 10",
                "pr_title": "chore: forge loop cycle 10",
                "prompt_sha256": "sha256-not-the-real-prompt",
                "once": true,
                "auto_merge": true,
                "strict_upgrade_only": true,
                "phases": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]
            }"#,
        )
        .expect("manifest");

        let error = parse_cycle_manifest(&path).expect_err("forged prompt hash witness must fail");
        assert!(
            error.root_cause().to_string().contains("prompt_sha256"),
            "error should name the invalid manifest witness: {error}"
        );

        fs::remove_file(path).ok();
    }

    #[test]
    fn eval_manifest_rejects_schema_version_mismatch() {
        let path = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-bad-schema-{}.json",
            std::process::id()
        ));
        fs::write(
            &path,
            r#"{
                "schema_version": 2,
                "goal": "upgrade manifest schema witness",
                "pr_title": "chore: forge loop self-upgrade",
                "prompt_sha256": "ignored",
                "once": true,
                "auto_merge": true,
                "strict_upgrade_only": true,
                "phases": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]
            }"#,
        )
        .expect("manifest");

        let error = parse_cycle_manifest(&path).expect_err("schema mismatch must fail");
        assert!(
            error.root_cause().to_string().contains("schema_version"),
            "error should name the unsupported schema version: {error}"
        );

        fs::remove_file(path).ok();
    }

    #[test]
    fn cycle_prompt_binds_nested_codex_to_single_cycle_and_pr_title() {
        let prompt = cycle_prompt(
            "Resume the interrupted 10-cycle objective: execute isolated cycle 07 of 10",
            true,
        );

        assert!(prompt.contains("Do not start another cycle."));
        assert!(prompt.contains("PR title 'chore: forge loop cycle 07'"));
    }

    #[test]
    fn runner_health_flags_pending_local_runner_checks() {
        let payload = CheckRollupPayload {
            status_check_rollup: vec![
                CheckRollupEntry {
                    name: "Local Linux CI".into(),
                    status: "QUEUED".into(),
                    conclusion: String::new(),
                },
                CheckRollupEntry {
                    name: "Semantic PR Title".into(),
                    status: "IN_PROGRESS".into(),
                    conclusion: String::new(),
                },
                CheckRollupEntry {
                    name: "Analyze (rust)".into(),
                    status: "COMPLETED".into(),
                    conclusion: "SUCCESS".into(),
                },
            ],
        };

        let report = classify_runner_health(&payload.status_check_rollup);

        assert!(report.runner_pressure);
        assert_eq!(
            report.pending_local_checks,
            vec![
                "Local Linux CI".to_string(),
                "Semantic PR Title".to_string()
            ]
        );
        assert!(report.recommendation.contains("self-hosted runner"));
    }

    #[test]
    fn runner_health_accepts_gh_pr_view_status_check_rollup_json() {
        let payload = r#"{
            "statusCheckRollup": [
                {"name": "Local Linux CI", "status": "COMPLETED", "conclusion": "SUCCESS"},
                {"name": "Semantic PR Title", "status": "COMPLETED", "conclusion": "SUCCESS"}
            ]
        }"#;

        let parsed = parse_check_rollup(payload).expect("gh pr view payload");
        let report = classify_runner_health(&parsed.status_check_rollup);

        assert!(!report.runner_pressure);
        assert!(report.pending_local_checks.is_empty());
        assert_eq!(
            report.passed_local_checks,
            vec![
                "Local Linux CI".to_string(),
                "Semantic PR Title".to_string()
            ]
        );
    }

    #[test]
    fn runner_health_prefers_pending_replacement_over_cancelled_duplicate() {
        let payload = CheckRollupPayload {
            status_check_rollup: vec![
                CheckRollupEntry {
                    name: "Semantic PR Title".into(),
                    status: "COMPLETED".into(),
                    conclusion: "CANCELLED".into(),
                },
                CheckRollupEntry {
                    name: "Semantic PR Title".into(),
                    status: "QUEUED".into(),
                    conclusion: String::new(),
                },
            ],
        };

        let report = classify_runner_health(&payload.status_check_rollup);

        assert_eq!(
            report.pending_local_checks,
            vec!["Semantic PR Title".to_string()]
        );
        assert!(report.failed_local_checks.is_empty());
        assert!(report.runner_pressure);
    }

    #[test]
    fn runner_health_prefers_success_replacement_over_cancelled_duplicate() {
        let payload = CheckRollupPayload {
            status_check_rollup: vec![
                CheckRollupEntry {
                    name: "Semantic PR Title".into(),
                    status: "COMPLETED".into(),
                    conclusion: "CANCELLED".into(),
                },
                CheckRollupEntry {
                    name: "Semantic PR Title".into(),
                    status: "COMPLETED".into(),
                    conclusion: "SUCCESS".into(),
                },
            ],
        };

        let report = classify_runner_health(&payload.status_check_rollup);

        assert_eq!(
            report.passed_local_checks,
            vec!["Semantic PR Title".to_string()]
        );
        assert!(report.failed_local_checks.is_empty());
        assert!(!report.runner_pressure);
    }

    #[test]
    fn runner_health_flags_missing_required_local_checks() {
        let payload = CheckRollupPayload {
            status_check_rollup: vec![CheckRollupEntry {
                name: "Local Linux CI".into(),
                status: "COMPLETED".into(),
                conclusion: "SUCCESS".into(),
            }],
        };

        let report = classify_runner_health(&payload.status_check_rollup);

        assert_eq!(
            report.missing_local_checks,
            vec!["Semantic PR Title".to_string()]
        );
        assert!(report.recommendation.contains("required local checks"));
    }

    #[test]
    fn runner_health_reports_required_local_check_contract() {
        let report = classify_runner_health(&[]);

        assert_eq!(
            report.required_local_checks,
            vec![
                "Local Linux CI".to_string(),
                "Semantic PR Title".to_string()
            ]
        );
        assert_eq!(report.missing_local_checks, report.required_local_checks);
    }

    #[test]
    fn dry_run_writes_research_sources_artifact() {
        let out = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-research-artifact-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&out).ok();

        run(RunArgs {
            goal: "cycle 15 research artifact witness".into(),
            out: out.clone(),
            dry_run: true,
            auto_merge: true,
            once: true,
        })
        .expect("dry run");

        let cycle_dir = fs::read_dir(&out)
            .expect("artifact root")
            .next()
            .expect("one cycle artifact")
            .expect("cycle dir")
            .path();
        let sources = fs::read_to_string(cycle_dir.join("research-sources.json"))
            .expect("research sources artifact");
        let parsed: serde_json::Value = serde_json::from_str(&sources).expect("research sources");
        let ids = parsed
            .as_array()
            .expect("research source array")
            .iter()
            .filter_map(|source| source.get("id"))
            .filter_map(|id| id.as_str())
            .collect::<Vec<_>>();

        assert!(ids.contains(&"openai-codex"));
        assert!(ids.contains(&"kclaw0"));

        fs::remove_dir_all(out).ok();
    }

    #[test]
    fn doctor_json_exports_required_gate_contract() {
        let report = serde_json::json!({
            "required_gate_commands": REQUIRED_GATE_COMMANDS,
        });
        let gates = report["required_gate_commands"]
            .as_array()
            .expect("gate commands");

        assert!(gates
            .iter()
            .any(|gate| gate == "cargo audit --deny warnings"));
        assert!(gates
            .iter()
            .any(|gate| gate == "cargo run -q -p runner-cli -- forge-loop docs-drift --json"));
    }

    #[test]
    fn components_audit_reports_present_and_missing_loop_surfaces() {
        let out = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-components-audit-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&out).ok();
        fs::create_dir_all(out.join(".codex/prompts")).expect("prompt dir");
        fs::create_dir_all(out.join(".agents/skills/forge-loop-research")).expect("skill dir");
        fs::write(out.join(".codex/prompts/forge-loop.md"), "prompt").expect("prompt");
        fs::write(
            out.join(".agents/skills/forge-loop-research/SKILL.md"),
            "skill",
        )
        .expect("skill");

        let report = components_audit_report(&out);

        assert_eq!(report.checked_components, 28);
        assert!(report
            .present_components
            .contains(&"codex-prompt".to_string()));
        assert!(report.present_components.contains(&"skill".to_string()));
        assert!(report
            .missing_components
            .contains(&"project-config".to_string()));
        assert!(report.missing_components.contains(&"hooks".to_string()));

        fs::remove_dir_all(out).ok();
    }

    #[test]
    fn runner_flow_audit_reports_idle_and_sustain_evidence() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .to_path_buf();
        let report = runner_flow_audit_report(&RunnerFlowAuditArgs {
            root,
            runs_json: None,
            prs_json: None,
            json: true,
            strict: false,
        })
        .expect("runner flow report");

        assert!(report.sustain_workflow_present);
        assert!(report.idle_without_work);
        assert!(report
            .missing_evidence
            .contains(&"active_or_queued_runner_work"));
        assert!(report.kclaw0_target.contains("24/7 dark-factory"));
    }

    #[test]
    fn runner_flow_audit_accepts_active_work_and_clean_pr_flow() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .to_path_buf();
        let temp =
            std::env::temp_dir().join(format!("fxrun-runner-flow-audit-{}", std::process::id()));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let runs = temp.join("runs.json");
        let prs = temp.join("prs.json");
        fs::write(
            &runs,
            r#"[{"name":"CI","status":"in_progress","conclusion":"","headBranch":"main","event":"push","url":"https://example.invalid"}]"#,
        )
        .expect("runs json");
        fs::write(
            &prs,
            r#"[{"statusCheckRollup":[{"name":"Local Linux CI","status":"COMPLETED","conclusion":"SUCCESS"},{"name":"Semantic PR Title","status":"COMPLETED","conclusion":"SUCCESS"}]}]"#,
        )
        .expect("prs json");

        let report = runner_flow_audit_report(&RunnerFlowAuditArgs {
            root,
            runs_json: Some(runs),
            prs_json: Some(prs),
            json: true,
            strict: false,
        })
        .expect("runner flow report");

        assert_eq!(report.active_runs, 1);
        assert!(!report.idle_without_work);
        assert!(report.pr_flow_seamless);
        assert!(report.missing_evidence.is_empty());
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_black_factor_audit_requires_observed_window_sustain_and_pr_flow() {
        let temp = std::env::temp_dir().join(format!(
            "fxrun-runner-black-factor-fail-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let runs = temp.join("runs.json");
        let prs = temp.join("prs.json");
        fs::write(
            &runs,
            r#"[{"name":"Runner Sustain","status":"completed","conclusion":"success","createdAt":"2026-06-27T00:00:00Z"}]"#,
        )
        .expect("runs json");
        fs::write(&prs, r#"[]"#).expect("prs json");

        let report = runner_black_factor_audit_report(&RunnerBlackFactorAuditArgs {
            runs_json: runs,
            prs_json: prs,
            min_window_hours: 12,
            min_sustain_runs: 72,
            min_sustain_duration_minutes: 5,
            min_clean_merged_prs: 1,
            json: true,
            strict: false,
        })
        .expect("black factor report");

        assert!(!report.exceeded);
        assert!(report.missing_evidence.contains(&"observed_12h_window"));
        assert!(report.missing_evidence.contains(&"sustain_run_count"));
        assert_eq!(report.remaining_sustain_runs, 72);
        assert_eq!(report.min_minutes_to_sustain_target, 360);
        assert_eq!(report.short_or_unproven_sustain_runs, 1);
        assert!(report.missing_evidence.contains(&"clean_merged_pr_flow"));
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_black_factor_audit_accepts_kclaw0_window_fixture() {
        let temp = std::env::temp_dir().join(format!(
            "fxrun-runner-black-factor-pass-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let runs = temp.join("runs.json");
        let prs = temp.join("prs.json");
        let mut run_items = Vec::new();
        for step in 0..72 {
            let minute = step * 10;
            run_items.push(format!(
                r#"{{"name":"Runner Sustain","status":"completed","conclusion":"success","createdAt":"2026-06-27T{:02}:{:02}:00Z","updatedAt":"2026-06-27T{:02}:{:02}:00Z"}}"#,
                minute / 60,
                minute % 60,
                (minute + 6) / 60,
                (minute + 6) % 60
            ));
        }
        run_items.push(
            r#"{"name":"CI","status":"completed","conclusion":"success","createdAt":"2026-06-27T12:00:00Z","updatedAt":"2026-06-27T12:01:00Z"}"#.to_string(),
        );
        fs::write(&runs, format!("[{}]", run_items.join(","))).expect("runs json");
        fs::write(
            &prs,
            r#"[{"state":"MERGED","mergedAt":"2026-06-27T13:05:00Z","statusCheckRollup":[{"name":"Local Linux CI","status":"COMPLETED","conclusion":"SUCCESS"},{"name":"Semantic PR Title","status":"COMPLETED","conclusion":"SUCCESS"}]}]"#,
        )
        .expect("prs json");

        let report = runner_black_factor_audit_report(&RunnerBlackFactorAuditArgs {
            runs_json: runs,
            prs_json: prs,
            min_window_hours: 12,
            min_sustain_runs: 72,
            min_sustain_duration_minutes: 5,
            min_clean_merged_prs: 1,
            json: true,
            strict: false,
        })
        .expect("black factor report");

        assert!(report.exceeded, "{:?}", report.missing_evidence);
        assert!(report.observed_window_minutes >= 12 * 60);
        assert_eq!(report.successful_sustain_runs, 72);
        assert_eq!(report.total_duration_proven_sustain_runs, 72);
        assert_eq!(report.remaining_sustain_runs, 0);
        assert_eq!(report.min_minutes_to_sustain_target, 0);
        assert_eq!(report.short_or_unproven_sustain_runs, 0);
        assert_eq!(report.clean_merged_prs, 1);
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_black_factor_audit_counts_only_latest_proof_window() {
        let temp = std::env::temp_dir().join(format!(
            "fxrun-runner-black-factor-rolling-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let runs = temp.join("runs.json");
        let prs = temp.join("prs.json");
        let mut run_items = Vec::new();
        for step in 0..72 {
            let minute = step * 10;
            run_items.push(format!(
                r#"{{"name":"Runner Sustain","status":"completed","conclusion":"success","createdAt":"2026-06-26T{:02}:{:02}:00Z","updatedAt":"2026-06-26T{:02}:{:02}:00Z"}}"#,
                minute / 60,
                minute % 60,
                (minute + 6) / 60,
                (minute + 6) % 60
            ));
        }
        run_items.push(
            r#"{"name":"CI","status":"completed","conclusion":"success","createdAt":"2026-06-27T12:00:00Z","updatedAt":"2026-06-27T12:01:00Z"}"#.to_string(),
        );
        fs::write(&runs, format!("[{}]", run_items.join(","))).expect("runs json");
        fs::write(
            &prs,
            r#"[{"state":"MERGED","mergedAt":"2026-06-27T12:05:00Z","statusCheckRollup":[{"name":"Local Linux CI","status":"COMPLETED","conclusion":"SUCCESS"},{"name":"Semantic PR Title","status":"COMPLETED","conclusion":"SUCCESS"}]}]"#,
        )
        .expect("prs json");

        let report = runner_black_factor_audit_report(&RunnerBlackFactorAuditArgs {
            runs_json: runs,
            prs_json: prs,
            min_window_hours: 12,
            min_sustain_runs: 72,
            min_sustain_duration_minutes: 5,
            min_clean_merged_prs: 1,
            json: true,
            strict: false,
        })
        .expect("black factor report");

        assert!(!report.exceeded);
        assert!(report.observed_window_minutes >= 12 * 60);
        assert_eq!(report.total_duration_proven_sustain_runs, 72);
        assert_eq!(report.successful_sustain_runs, 0);
        assert_eq!(report.remaining_sustain_runs, 72);
        assert_eq!(report.min_minutes_to_sustain_target, 360);
        assert!(report.missing_evidence.contains(&"sustain_run_count"));
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_black_factor_audit_rejects_short_yielded_sustain_runs() {
        let temp = std::env::temp_dir().join(format!(
            "fxrun-runner-black-factor-short-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let runs = temp.join("runs.json");
        let prs = temp.join("prs.json");
        let mut run_items = vec![
            r#"{"name":"Runner Sustain","status":"completed","conclusion":"success","createdAt":"2026-06-27T00:00:00Z","updatedAt":"2026-06-27T00:01:00Z"}"#.to_string(),
            r#"{"name":"Runner Sustain","status":"completed","conclusion":"success","createdAt":"2026-06-27T13:00:00Z","updatedAt":"2026-06-27T13:01:00Z"}"#.to_string(),
        ];
        for minute in 1..=71 {
            run_items.push(format!(
                r#"{{"name":"Runner Sustain","status":"completed","conclusion":"success","createdAt":"2026-06-27T{:02}:{:02}:00Z","updatedAt":"2026-06-27T{:02}:{:02}:00Z"}}"#,
                minute / 60,
                minute % 60,
                (minute + 1) / 60,
                (minute + 1) % 60
            ));
        }
        fs::write(&runs, format!("[{}]", run_items.join(","))).expect("runs json");
        fs::write(
            &prs,
            r#"[{"state":"MERGED","mergedAt":"2026-06-27T13:05:00Z","statusCheckRollup":[{"name":"Local Linux CI","status":"COMPLETED","conclusion":"SUCCESS"},{"name":"Semantic PR Title","status":"COMPLETED","conclusion":"SUCCESS"}]}]"#,
        )
        .expect("prs json");

        let report = runner_black_factor_audit_report(&RunnerBlackFactorAuditArgs {
            runs_json: runs,
            prs_json: prs,
            min_window_hours: 12,
            min_sustain_runs: 72,
            min_sustain_duration_minutes: 5,
            min_clean_merged_prs: 1,
            json: true,
            strict: false,
        })
        .expect("black factor report");

        assert!(!report.exceeded);
        assert_eq!(report.successful_sustain_runs, 0);
        assert_eq!(report.short_or_unproven_sustain_runs, 73);
        assert_eq!(report.remaining_sustain_runs, 72);
        assert_eq!(report.min_minutes_to_sustain_target, 360);
        assert!(report.missing_evidence.contains(&"sustain_run_count"));
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_ops_slo_audit_accepts_event_rehydrated_burn_in() {
        let temp =
            std::env::temp_dir().join(format!("fxrun-runner-ops-slo-pass-{}", std::process::id()));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let runs = temp.join("runs.json");
        let prs = temp.join("prs.json");
        let mut run_items = Vec::new();
        for minute in (0..=50).step_by(10) {
            run_items.push(format!(
                r#"{{"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","createdAt":"2026-06-27T00:{minute:02}:00Z","updatedAt":"2026-06-27T00:{:02}:00Z"}}"#,
                minute + 6
            ));
        }
        run_items.push(
            r#"{"name":"Runner Black Factor Watch","status":"completed","conclusion":"success","event":"workflow_run","createdAt":"2026-06-27T00:30:00Z","updatedAt":"2026-06-27T00:31:00Z"}"#.to_string(),
        );
        run_items.push(
            r#"{"name":"Runner Sustain","status":"in_progress","conclusion":"","event":"workflow_dispatch","createdAt":"2026-06-27T01:00:00Z","updatedAt":"2026-06-27T01:00:00Z"}"#.to_string(),
        );
        fs::write(&runs, format!("[{}]", run_items.join(","))).expect("runs json");
        fs::write(&prs, r#"[]"#).expect("prs json");

        let report = runner_ops_slo_audit_report(&RunnerOpsSloAuditArgs {
            runs_json: runs,
            prs_json: prs,
            min_window_hours: 1,
            max_idle_gap_minutes: 10,
            min_active_or_queued_sustain: 1,
            min_event_watch_wakeups: 1,
            max_failed_ops_runs: 0,
            min_sustain_duration_minutes: 5,
            json: true,
            strict: false,
        })
        .expect("ops slo report");

        assert!(report.burn_in_ready, "{:?}", report.missing_evidence);
        assert_eq!(report.observed_window_minutes, 60);
        assert_eq!(report.max_idle_gap_minutes_observed, 4);
        assert_eq!(report.active_or_queued_sustain_runs, 1);
        assert_eq!(report.event_watch_wakeups, 1);
        assert_eq!(report.failed_ops_runs, 0);
        assert!(report.pr_flow_seamless);
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_ops_slo_audit_accepts_explicit_sustain_completion_watch_wakeup() {
        let temp = std::env::temp_dir().join(format!(
            "fxrun-runner-ops-slo-explicit-watch-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let runs = temp.join("runs.json");
        let prs = temp.join("prs.json");
        fs::write(
            &runs,
            r#"[
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","createdAt":"2026-06-27T00:00:00Z","updatedAt":"2026-06-27T00:06:00Z"},
              {"name":"Runner Black Factor Watch (workflow_dispatch sustain_completion)","status":"completed","conclusion":"success","event":"workflow_dispatch","displayTitle":"Runner Black Factor Watch (workflow_dispatch sustain_completion)","createdAt":"2026-06-27T00:30:00Z","updatedAt":"2026-06-27T00:31:00Z"},
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","createdAt":"2026-06-27T00:40:00Z","updatedAt":"2026-06-27T00:46:00Z"},
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","createdAt":"2026-06-27T00:50:00Z","updatedAt":"2026-06-27T00:56:00Z"},
              {"name":"Runner Sustain","status":"queued","conclusion":"","event":"workflow_dispatch","createdAt":"2026-06-27T01:00:00Z","updatedAt":"2026-06-27T01:00:00Z"}
            ]"#,
        )
        .expect("runs json");
        fs::write(&prs, r#"[]"#).expect("prs json");

        let report = runner_ops_slo_audit_report(&RunnerOpsSloAuditArgs {
            runs_json: runs,
            prs_json: prs,
            min_window_hours: 1,
            max_idle_gap_minutes: 40,
            min_active_or_queued_sustain: 1,
            min_event_watch_wakeups: 1,
            max_failed_ops_runs: 0,
            min_sustain_duration_minutes: 5,
            json: true,
            strict: false,
        })
        .expect("ops slo report");

        assert!(report.burn_in_ready, "{:?}", report.missing_evidence);
        assert_eq!(report.event_watch_wakeups, 1);
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_ops_slo_audit_treats_watch_run_names_as_ops_workflow() {
        assert!(is_ops_workflow("Runner Black Factor Watch"));
        assert!(is_ops_workflow(
            "Runner Black Factor Watch (workflow_run CI)"
        ));
        assert!(!is_ops_workflow("Runner Black Factor"));
    }

    #[test]
    fn runner_ops_slo_audit_reports_idle_failure_and_pr_pressure() {
        let temp =
            std::env::temp_dir().join(format!("fxrun-runner-ops-slo-fail-{}", std::process::id()));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let runs = temp.join("runs.json");
        let prs = temp.join("prs.json");
        fs::write(
            &runs,
            r#"[
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","createdAt":"2026-06-27T00:00:00Z","updatedAt":"2026-06-27T00:05:00Z"},
              {"name":"CI","status":"completed","conclusion":"failure","event":"push","createdAt":"2026-06-27T01:00:00Z","updatedAt":"2026-06-27T01:02:00Z"}
            ]"#,
        )
        .expect("runs json");
        fs::write(
            &prs,
            r#"[{"statusCheckRollup":[{"name":"Local Linux CI","status":"QUEUED","conclusion":""},{"name":"Semantic PR Title","status":"COMPLETED","conclusion":"SUCCESS"}]}]"#,
        )
        .expect("prs json");

        let report = runner_ops_slo_audit_report(&RunnerOpsSloAuditArgs {
            runs_json: runs,
            prs_json: prs,
            min_window_hours: 1,
            max_idle_gap_minutes: 10,
            min_active_or_queued_sustain: 1,
            min_event_watch_wakeups: 1,
            max_failed_ops_runs: 0,
            min_sustain_duration_minutes: 5,
            json: true,
            strict: false,
        })
        .expect("ops slo report");

        assert!(!report.burn_in_ready);
        assert!(report.max_idle_gap_minutes_observed > 10);
        assert_eq!(report.active_or_queued_sustain_runs, 0);
        assert_eq!(report.event_watch_wakeups, 0);
        assert_eq!(report.failed_ops_runs, 1);
        assert_eq!(report.queued_required_checks, 1);
        assert!(!report.pr_flow_seamless);
        for expected in [
            "idle_gap_slo",
            "active_or_queued_sustain_backlog",
            "event_watch_rehydration",
            "failed_ops_budget",
            "seamless_pr_flow",
        ] {
            assert!(
                report.missing_evidence.contains(&expected),
                "missing {expected}: {:?}",
                report.missing_evidence
            );
        }
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_ops_slo_audit_counts_pr_checks_as_productive_runner_work() {
        let temp = std::env::temp_dir().join(format!(
            "fxrun-runner-ops-slo-pr-work-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let runs = temp.join("runs.json");
        let prs = temp.join("prs.json");
        fs::write(
            &runs,
            r#"[
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T00:00:00Z","updatedAt":"2026-06-27T00:05:00Z"},
              {"name":"CI","status":"completed","conclusion":"success","event":"pull_request","headBranch":"feature","createdAt":"2026-06-27T00:10:00Z","updatedAt":"2026-06-27T00:25:00Z"},
              {"name":"Semantic PR Title","status":"completed","conclusion":"success","event":"pull_request_target","headBranch":"feature","createdAt":"2026-06-27T00:30:00Z","updatedAt":"2026-06-27T00:32:00Z"},
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T00:40:00Z","updatedAt":"2026-06-27T00:46:00Z"},
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T00:50:00Z","updatedAt":"2026-06-27T00:56:00Z"},
              {"name":"Runner Black Factor Watch","status":"completed","conclusion":"success","event":"workflow_run","headBranch":"main","createdAt":"2026-06-27T00:57:00Z","updatedAt":"2026-06-27T00:58:00Z"},
              {"name":"Runner Sustain","status":"queued","conclusion":"","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T01:00:00Z","updatedAt":"2026-06-27T01:00:00Z"}
            ]"#,
        )
        .expect("runs json");
        fs::write(&prs, r#"[]"#).expect("prs json");

        let report = runner_ops_slo_audit_report(&RunnerOpsSloAuditArgs {
            runs_json: runs,
            prs_json: prs,
            min_window_hours: 1,
            max_idle_gap_minutes: 10,
            min_active_or_queued_sustain: 1,
            min_event_watch_wakeups: 1,
            max_failed_ops_runs: 0,
            min_sustain_duration_minutes: 5,
            json: true,
            strict: false,
        })
        .expect("ops slo report");

        assert!(report.burn_in_ready, "{:?}", report.missing_evidence);
        assert!(report.max_idle_gap_minutes_observed <= 10);
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_ops_slo_audit_ignores_superseded_cancellations_with_nearby_success() {
        let temp = std::env::temp_dir().join(format!(
            "fxrun-runner-ops-slo-cancel-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let runs = temp.join("runs.json");
        let prs = temp.join("prs.json");
        fs::write(
            &runs,
            r#"[
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T00:00:00Z","updatedAt":"2026-06-27T00:06:00Z"},
              {"name":"Semantic PR Title","status":"completed","conclusion":"cancelled","event":"pull_request_target","headBranch":"feature","createdAt":"2026-06-27T00:10:00Z","updatedAt":"2026-06-27T00:10:30Z"},
              {"name":"Semantic PR Title","status":"completed","conclusion":"success","event":"pull_request_target","headBranch":"feature","createdAt":"2026-06-27T00:11:00Z","updatedAt":"2026-06-27T00:12:00Z"},
              {"name":"Runner Black Factor Watch","status":"completed","conclusion":"success","event":"workflow_run","headBranch":"main","createdAt":"2026-06-27T00:30:00Z","updatedAt":"2026-06-27T00:31:00Z"},
              {"name":"Runner Sustain","status":"in_progress","conclusion":"","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T01:00:00Z","updatedAt":"2026-06-27T01:00:00Z"}
            ]"#,
        )
        .expect("runs json");
        fs::write(&prs, r#"[]"#).expect("prs json");

        let report = runner_ops_slo_audit_report(&RunnerOpsSloAuditArgs {
            runs_json: runs,
            prs_json: prs,
            min_window_hours: 1,
            max_idle_gap_minutes: 60,
            min_active_or_queued_sustain: 1,
            min_event_watch_wakeups: 1,
            max_failed_ops_runs: 0,
            min_sustain_duration_minutes: 5,
            json: true,
            strict: false,
        })
        .expect("ops slo report");

        assert_eq!(report.failed_ops_runs, 0);
        assert!(!report.missing_evidence.contains(&"failed_ops_budget"));
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_fleet_audit_flags_external_repo_lane_pressure() {
        let temp =
            std::env::temp_dir().join(format!("fxrun-runner-fleet-audit-{}", std::process::id()));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let jobs = temp.join("jobs.json");
        fs::write(
            &jobs,
            r#"[
              {"repository":"FlexNetOS/flexnetos_runner","workflow":"Runner Sustain","run_id":"1","job":"local-runner-sustain","workspace":"/runner/_work/flexnetos_runner","pids":[10,11]},
              {"repository":"FlexNetOS/meta","workflow":"CI","run_id":"2","job":"integration","workspace":"/runner/_work/meta","head_ref":"chore/remove-empty-repos","pids":[20,21]},
              {"repository":"FlexNetOS/meta","workflow":"CI","run_id":"2","job":"integration","workspace":"/runner/_work/meta","head_ref":"chore/remove-empty-repos","pids":[22]}
            ]"#,
        )
        .expect("jobs json");

        let report = runner_fleet_audit_report(&RunnerFleetAuditArgs {
            expected_repository: "FlexNetOS/flexnetos_runner".to_string(),
            jobs_json: Some(jobs),
            proc_root: PathBuf::from("/proc"),
            max_external_jobs: 0,
            json: true,
            strict: false,
        })
        .expect("fleet audit");

        assert!(!report.fleet_ready);
        assert_eq!(report.total_jobs, 2);
        assert_eq!(report.expected_repository_jobs, 1);
        assert_eq!(report.external_repository_jobs, 1);
        assert_eq!(report.external_repositories.get("FlexNetOS/meta"), Some(&1));
        assert!(
            report
                .missing_evidence
                .contains(&"external_runner_lane_pressure"),
            "{:?}",
            report.missing_evidence
        );
        let meta_job = report
            .jobs
            .iter()
            .find(|job| job.repository == "FlexNetOS/meta")
            .expect("meta job");
        assert_eq!(meta_job.pids, vec![20, 21, 22]);
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_fleet_audit_passes_when_only_expected_repo_owns_lanes() {
        let temp = std::env::temp_dir().join(format!(
            "fxrun-runner-fleet-audit-pass-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let jobs = temp.join("jobs.json");
        fs::write(
            &jobs,
            r#"[
              {"repository":"FlexNetOS/flexnetos_runner","workflow":"Runner Sustain","run_id":"1","job":"local-runner-sustain","workspace":"/runner/_work/flexnetos_runner","pids":[10]}
            ]"#,
        )
        .expect("jobs json");

        let report = runner_fleet_audit_report(&RunnerFleetAuditArgs {
            expected_repository: "FlexNetOS/flexnetos_runner".to_string(),
            jobs_json: Some(jobs),
            proc_root: PathBuf::from("/proc"),
            max_external_jobs: 0,
            json: true,
            strict: false,
        })
        .expect("fleet audit");

        assert!(report.fleet_ready, "{:?}", report.missing_evidence);
        assert_eq!(report.total_jobs, 1);
        assert_eq!(report.external_repository_jobs, 0);
        assert!(report.external_repositories.is_empty());
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn agentic_system_audit_accepts_composed_live_proof() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .to_path_buf();
        let temp =
            std::env::temp_dir().join(format!("fxrun-agentic-system-audit-{}", std::process::id()));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let runs = temp.join("runs.json");
        let open_prs = temp.join("open-prs.json");
        let history_prs = temp.join("history-prs.json");
        let jobs = temp.join("jobs.json");
        fs::write(
            &runs,
            r#"[
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","createdAt":"2026-06-27T00:00:00Z","updatedAt":"2026-06-27T00:05:00Z"},
              {"name":"Runner Black Factor Watch","status":"completed","conclusion":"success","event":"workflow_run","displayTitle":"Runner Black Factor Watch (workflow_run Runner Sustain)","createdAt":"2026-06-27T00:06:00Z","updatedAt":"2026-06-27T00:07:00Z"},
              {"name":"CI","status":"completed","conclusion":"success","event":"pull_request","headBranch":"feature","createdAt":"2026-06-27T00:10:00Z","updatedAt":"2026-06-27T00:25:00Z"},
              {"name":"Semantic PR Title","status":"completed","conclusion":"success","event":"pull_request_target","headBranch":"feature","createdAt":"2026-06-27T00:11:00Z","updatedAt":"2026-06-27T00:12:00Z"},
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","createdAt":"2026-06-27T01:00:00Z","updatedAt":"2026-06-27T01:05:00Z"},
              {"name":"Runner Sustain","status":"in_progress","conclusion":"","event":"workflow_dispatch","createdAt":"2026-06-27T01:05:00Z","updatedAt":"2026-06-27T01:05:00Z"},
              {"name":"Runner Sustain","status":"queued","conclusion":"","event":"workflow_dispatch","createdAt":"2026-06-27T01:06:00Z","updatedAt":"2026-06-27T01:06:00Z"}
            ]"#,
        )
        .expect("runs json");
        fs::write(&open_prs, r#"[]"#).expect("open prs json");
        fs::write(
            &history_prs,
            r#"[{"state":"MERGED","mergedAt":"2026-06-27T00:30:00Z","statusCheckRollup":[{"name":"Local Linux CI","status":"COMPLETED","conclusion":"SUCCESS"},{"name":"Semantic PR Title","status":"COMPLETED","conclusion":"SUCCESS"}]}]"#,
        )
        .expect("history prs json");
        fs::write(
            &jobs,
            r#"[{"repository":"FlexNetOS/flexnetos_runner","workflow":"Runner Sustain","run_id":"1","job":"local-runner-sustain","workspace":"/runner/_work/flexnetos_runner","pids":[10]}]"#,
        )
        .expect("jobs json");

        let report = agentic_system_audit_report(&AgenticSystemAuditArgs {
            root,
            runs_json: Some(runs),
            open_prs_json: Some(open_prs),
            prs_history_json: Some(history_prs),
            expected_repository: "FlexNetOS/flexnetos_runner".to_string(),
            fleet_jobs_json: Some(jobs),
            proc_root: PathBuf::from("/proc"),
            min_window_hours: 1,
            min_slo_window_hours: 1,
            max_idle_gap_minutes: 120,
            min_active_or_queued_sustain: 1,
            min_event_watch_wakeups: 1,
            max_failed_ops_runs: 0,
            min_sustain_runs: 1,
            min_sustain_duration_minutes: 5,
            min_clean_merged_prs: 1,
            max_external_jobs: 0,
            json: true,
            strict: false,
        })
        .expect("agentic audit");

        assert!(report.end_to_end_ready, "{:?}", report.missing_evidence);
        assert!(report.research_loop_evidence);
        assert!(report.evaluation_loop_evidence);
        assert!(report.adaptation_loop_evidence);
        assert!(report.growth_loop_evidence);
        assert!(report.self_improvement_dispatch_evidence);
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn parse_nul_env_extracts_github_action_context() {
        let env = parse_nul_env(
            b"GITHUB_REPOSITORY=FlexNetOS/meta\0GITHUB_RUN_ID=28310752662\0IGNORED\0",
        );
        assert_eq!(
            env.get("GITHUB_REPOSITORY").map(String::as_str),
            Some("FlexNetOS/meta")
        );
        assert_eq!(
            env.get("GITHUB_RUN_ID").map(String::as_str),
            Some("28310752662")
        );
        assert!(!env.contains_key("IGNORED"));
    }

    #[test]
    fn runner_fleet_proc_helpers_require_runner_worker_ancestry() {
        assert_eq!(
            parse_status_ppid("Name:\tbash\nPPid:\t1132559\nState:\tS\n"),
            Some(1132559)
        );

        let mut index = BTreeMap::new();
        index.insert(
            1,
            ProcInfo {
                ppid: 0,
                cmdline: "systemd".to_string(),
            },
        );
        index.insert(
            10,
            ProcInfo {
                ppid: 1,
                cmdline: "/runner/bin/Runner.Worker spawnclient".to_string(),
            },
        );
        index.insert(
            11,
            ProcInfo {
                ppid: 10,
                cmdline: "bash /runner/_work/_temp/script.sh".to_string(),
            },
        );
        index.insert(
            12,
            ProcInfo {
                ppid: 1,
                cmdline: "claude --version".to_string(),
            },
        );

        assert!(has_runner_worker_ancestor(11, &index));
        assert!(!has_runner_worker_ancestor(12, &index));
    }

    #[test]
    fn rfc3339_parser_counts_minutes_between_github_timestamps() {
        let start = parse_rfc3339_utc_seconds("2026-06-27T00:00:00Z").expect("start");
        let end = parse_rfc3339_utc_seconds("2026-06-27T12:30:00Z").expect("end");
        assert_eq!((end - start) / 60, 750);
        assert!(parse_rfc3339_utc_seconds("not-a-date").is_none());
    }

    #[test]
    fn expected_loop_components_cover_requested_upgrade_surfaces() {
        let surfaces = expected_loop_components()
            .into_iter()
            .map(|component| component.surface)
            .collect::<Vec<_>>();

        for required in [
            "prompt",
            "config",
            "hooks",
            "rules",
            "agents",
            "permissions",
            "skills",
            "tools",
            "worktrees",
            "checklists",
            "docs",
        ] {
            assert!(
                surfaces.contains(&required),
                "missing component surface {required}"
            );
        }
    }

    #[test]
    fn forge_loop_config_enables_auto_compaction() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let config = fs::read_to_string(root.join(".codex/config.toml")).expect("read config");
        let compact_prompt =
            fs::read_to_string(root.join(COMPACT_PROMPT_PATH)).expect("read compact prompt");
        let workflow = fs::read_to_string(root.join(".github/workflows/codex-forge-loop.yml"))
            .expect("read Codex workflow");

        for required in [
            "auto_compaction = true",
            "model_auto_compact_token_limit = 3000000",
            "model_auto_compact_token_limit_scope = \"total\"",
            "tool_output_token_limit = 12000",
            "experimental_compact_prompt_file = \"prompts/compact-forge-loop.md\"",
        ] {
            assert!(config.contains(required), "config missing {required}");
        }
        assert!(compact_prompt.contains("active phase"));
        assert!(compact_prompt.contains("next action"));
        assert!(workflow.contains("features.auto_compaction=true"));
        assert!(workflow.contains("model_auto_compact_token_limit=3000000"));
    }

    #[test]
    fn stop_and_compact_hooks_preserve_phase_source_validation_next_action() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let stop_hook = fs::read_to_string(root.join(".codex/hooks/forge_loop_stop_summary.py"))
            .expect("read stop hook");
        let compact_hook =
            fs::read_to_string(root.join(".codex/hooks/forge_loop_compact_summary.py"))
                .expect("read compact hook");
        let compact_prompt =
            fs::read_to_string(root.join(COMPACT_PROMPT_PATH)).expect("read compact prompt");
        let output_schema =
            fs::read_to_string(root.join(".github/codex/schemas/forge-loop-output.schema.json"))
                .expect("read output schema");

        for required in [
            "active_phase",
            "source_coverage",
            "validation_state",
            "next_action",
        ] {
            assert!(stop_hook.contains(required), "stop hook missing {required}");
            assert!(
                compact_hook.contains(required),
                "compact hook missing {required}"
            );
            assert!(
                output_schema.contains(required),
                "output schema missing {required}"
            );
        }
        for required in ["active phase", "source", "validation", "next action"] {
            assert!(
                compact_prompt.contains(required),
                "compact prompt missing {required}"
            );
        }
    }

    #[test]
    fn forge_loop_cycle_evidence_checklist_requires_merge_proof() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let checklist = fs::read_to_string(root.join(".codex/checklists/forge-loop-cycle.toml"))
            .expect("read cycle checklist");
        let prompt = fs::read_to_string(root.join(".codex/prompts/forge-loop.md"))
            .expect("read forge-loop prompt");

        for required in [
            "strict_upgrade_only = true",
            "commit_push_pr_required = true",
            "component_audit",
            "target_mining_audit",
            "forge_loop_tests",
            "required_checks_green = true",
            "merged_at = true",
            "main_fast_forwarded = true",
        ] {
            assert!(
                checklist.contains(required),
                "cycle checklist missing {required}"
            );
        }
        assert!(prompt.contains(".codex/checklists/forge-loop-cycle.toml"));
    }

    #[test]
    fn forge_loop_hook_manifest_covers_registered_events() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let manifest = fs::read_to_string(root.join(".codex/hooks/forge-loop-hooks.manifest.json"))
            .expect("read hook manifest");
        let hooks = fs::read_to_string(root.join(".codex/hooks.json")).expect("read hooks");

        for required in [
            "SessionStart",
            "PreToolUse",
            "PermissionRequest",
            "PostToolUse",
            "PreCompact",
            "PostCompact",
            "SubagentStart",
            "SubagentStop",
            "Stop",
            "expected_json_key",
        ] {
            assert!(
                manifest.contains(required),
                "hook manifest missing {required}"
            );
        }
        for script in [
            "forge_loop_session_start.py",
            "forge_loop_pre_tool_use.py",
            "forge_loop_permission_request.py",
            "forge_loop_post_tool_use.py",
            "forge_loop_compact_summary.py",
            "forge_loop_subagent_summary.py",
            "forge_loop_stop_summary.py",
        ] {
            assert!(manifest.contains(script), "hook manifest missing {script}");
            assert!(hooks.contains(script), "hooks.json missing {script}");
        }
    }

    #[test]
    fn forge_loop_worktree_isolation_contract_is_present() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let contract = fs::read_to_string(root.join(".codex/worktrees/forge-loop-isolation.toml"))
            .expect("read worktree contract");
        let prompt = fs::read_to_string(root.join(".codex/prompts/forge-loop.md"))
            .expect("read forge-loop prompt");

        for required in [
            "required = true",
            "wait_for_merge_before_next_cycle = true",
            "forbid_shared_mutating_checkout = true",
            "components_audit = true",
            "target_mining_audit = true",
        ] {
            assert!(
                contract.contains(required),
                "worktree contract missing {required}"
            );
        }
        assert!(prompt.contains(".codex/worktrees/forge-loop-isolation.toml"));
    }

    #[test]
    fn forge_loop_skill_references_codex_config_and_action_docs() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let skill = fs::read_to_string(root.join(".agents/skills/forge-loop-research/SKILL.md"))
            .expect("read forge-loop skill");

        for required in [
            "https://developers.openai.com/codex/config-advanced",
            "https://developers.openai.com/codex/github-action",
            "https://developers.openai.com/codex/permissions",
            "https://developers.openai.com/codex/subagents",
            "components-audit",
            "model flags",
            "custom agents/subagents",
            "permission profiles",
            "structured output schemas",
        ] {
            assert!(skill.contains(required), "skill missing {required}");
        }
    }

    #[test]
    fn codex_github_action_workflow_uses_documented_controls() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let workflow = fs::read_to_string(root.join(".github/workflows/codex-forge-loop.yml"))
            .expect("read Codex workflow");

        assert!(
            !workflow.contains("if: ${{ secrets."),
            "GitHub Actions does not allow secrets in job-level if expressions"
        );

        for required in [
            "runs-on: [self-hosted, linux, x64, local, flexnetos]",
            "openai/codex-action@v1",
            "prompt-file:",
            "codex-args:",
            "--output-schema",
            ".github/codex/schemas/forge-loop-output.schema.json",
            "model:",
            "effort:",
            "sandbox: workspace-write",
            "safety-strategy: drop-sudo",
            "allow-bots:",
            "output-file:",
            "CODEX_HOME:",
            "login status",
            "codex exec",
            "--ignore-user-config",
            "--output-last-message",
            "GH_TOKEN:",
            "PROMPT_FILE_INPUT:",
            "invalid prompt_file input",
            "features.auto_compaction=true",
            "model_auto_compact_token_limit=3000000",
        ] {
            assert!(workflow.contains(required), "workflow missing {required}");
        }
    }

    #[test]
    fn agentic_system_watch_dispatches_codex_growth_safely() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let workflow = fs::read_to_string(root.join(".github/workflows/agentic-system-watch.yml"))
            .expect("read agentic system watch workflow");
        let proof = fs::read_to_string(root.join("docs/forge-loop/agentic-system-proof.md"))
            .expect("read agentic system proof doc");

        assert!(
            !workflow.contains("if: ${{ secrets."),
            "GitHub Actions does not allow secrets in job-level if expressions"
        );
        for required in [
            "name: Agentic System Watch",
            "*/30 * * * *",
            "workflow_run:",
            "Runner Black Factor Watch",
            "actions: write",
            "refreshing once after black-factor top-up settles",
            "agentic-system-audit",
            "--strict",
            "gh workflow run codex-forge-loop.yml",
            "local ChatGPT auth",
            "dispatch=codex_forge_loop",
            "skipped_open_pr",
            "skipped_pr_pressure",
            "skipped_active_codex",
            "agentic-dispatch.env",
        ] {
            assert!(workflow.contains(required), "workflow missing {required}");
        }
        for required in [
            "Always researching",
            "Always evaluating",
            "Always adapting",
            "Always growing",
            "Agentic System Watch",
            "Codex Forge Loop",
        ] {
            assert!(proof.contains(required), "proof doc missing {required}");
        }
    }

    #[test]
    fn codex_deep_target_mining_surfaces_are_guarded() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let config = fs::read_to_string(root.join(".codex/config.toml")).expect("read config");
        let hooks = fs::read_to_string(root.join(".codex/hooks.json")).expect("read hooks");
        let permissions =
            fs::read_to_string(root.join(".codex/permissions/forge-loop-workspace.toml"))
                .expect("read permission blueprint");
        let ledger = fs::read_to_string(root.join("docs/forge-loop/codex-target-mining.md"))
            .expect("read mining ledger");

        assert!(
            config.contains("sandbox_mode"),
            "active config should still use the older sandbox surface"
        );
        assert!(
            permissions.contains("default_permissions")
                && permissions.contains("**/*.env")
                && permissions.contains("developers.openai.com")
                && permissions.contains("github.com"),
            "permission blueprint must encode least-privilege filesystem and network intent"
        );
        assert!(
            !config.contains("default_permissions"),
            "do not mix active permission profiles with sandbox_mode"
        );
        for required in [
            "PreToolUse",
            "PermissionRequest",
            "PostToolUse",
            "PreCompact",
            "PostCompact",
            "SubagentStart",
            "SubagentStop",
            "forge_loop_pre_tool_use.py",
            "forge_loop_permission_request.py",
            "forge_loop_post_tool_use.py",
            "forge_loop_compact_summary.py",
            "forge_loop_subagent_summary.py",
        ] {
            assert!(hooks.contains(required), "hooks missing {required}");
        }
        for required in [
            "developers.openai.com/codex/github-action",
            "developers.openai.com/codex/permissions",
            "developers.openai.com/codex/subagents",
            "RoggeOhta/awesome-codex-cli",
            "Yeachan-Heo/oh-my-codex",
        ] {
            assert!(ledger.contains(required), "ledger missing {required}");
        }
    }

    #[test]
    fn target_mining_audit_proves_sources_applications_and_guards() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let report = target_mining_audit_report(root);

        assert_eq!(report.checked_targets, 7);
        assert!(
            report
                .covered_targets
                .iter()
                .any(|target| target == "kclaw0"),
            "kclaw0 target must be first-class target-mining coverage"
        );
        assert!(
            report
                .covered_targets
                .iter()
                .any(|target| target == "kclaw0-referenced-resources"),
            "kclaw0 referenced resources must be first-class target-mining coverage"
        );
        assert!(
            report.missing_targets.is_empty(),
            "target mining gaps: {:?}",
            report.missing_targets
        );
        for target in report.targets {
            assert!(
                target.source_evidence,
                "{} missing source evidence",
                target.id
            );
            assert!(
                target.application_evidence,
                "{} missing application evidence",
                target.id
            );
            assert!(
                target.guard_evidence,
                "{} missing guard evidence",
                target.id
            );
        }
    }

    #[test]
    fn ci_runs_components_audit_guard() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let ci =
            fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("read CI workflow");
        assert!(
            ci.contains("forge-loop components-audit --strict"),
            "CI must enforce the forge-loop component contract"
        );
        assert!(
            ci.contains("forge-loop target-mining-audit --strict"),
            "CI must enforce the forge-loop target mining contract"
        );
        assert!(
            ci.contains("forge-loop agentic-system-audit"),
            "CI must exercise the agentic-system audit surface"
        );
    }

    #[test]
    fn runner_sustain_workflow_bridges_schedule_interval() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let workflow = fs::read_to_string(root.join(".github/workflows/runner-sustain.yml"))
            .expect("read runner sustain workflow");
        let target = fs::read_to_string(root.join("docs/forge-loop/kclaw0-runner-flow-target.md"))
            .expect("read kclaw0 runner target");

        for required in [
            "duration_minutes",
            "default: '5'",
            "*/5 * * * *",
            "timeout-minutes: 10",
            "while [",
            "runner-sustain slot=",
            "tick_seconds",
            "default: '30'",
            "lane_slot",
            "runner-sustain-${{ github.ref }}-${{ github.run_id }}",
            "SLOT: ${{ inputs.lane_slot || '1' }}",
            "Yield to pull-request local checks",
            "yielding because PR pressure query failed",
            "workflow-run pressure query failed",
            "gh run list --limit 100 --json name,status",
            r#"case "$pr_pressure" in (*[!0-9]*|'') pr_pressure=1"#,
            "yielding mid-run",
            "gh pr list --state open",
            "actions: write",
            "Refill Runner Sustain backlog on completion",
            "MIN_SUSTAIN_BACKLOG: '4'",
            "dispatching Runner Sustain self-refill lane",
            "skipping Runner Sustain self-refill because",
            "gh workflow run runner-sustain.yml --ref main",
        ] {
            assert!(
                workflow.contains(required),
                "runner sustain bridge missing {required}"
            );
        }
        assert!(target.contains("Bridge-duration sustain policy"));
        assert!(target.contains("self-refill replacement"));
        assert!(target.contains("12+ hour kclaw0 persistence target"));
    }

    #[test]
    fn runner_black_factor_watch_refills_and_artifacts_evidence() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let workflow =
            fs::read_to_string(root.join(".github/workflows/runner-black-factor-watch.yml"))
                .expect("read runner black-factor watch workflow");
        let target = fs::read_to_string(root.join("docs/forge-loop/kclaw0-runner-flow-target.md"))
            .expect("read kclaw0 runner target");

        for required in [
            "Runner Black Factor Watch",
            "run-name: Runner Black Factor Watch",
            "trigger_source",
            "*/5 * * * *",
            "workflow_run:",
            "workflows:",
            "- CI",
            "- Semantic PR Title",
            "- Runner Sustain",
            "completed",
            "runs-on: ubuntu-latest",
            "actions: write",
            "MIN_SUSTAIN_BACKLOG: '4'",
            "target_sustain_backlog",
            "if [ \"$target_sustain_backlog\" -gt 4 ]; then target_sustain_backlog=4; fi",
            "gh workflow run runner-sustain.yml --ref main",
            "duration_minutes=5",
            "tick_seconds=30",
            "lane_slot=",
            "skipping Runner Sustain backlog top-up because",
            "pr_pending_pressure",
            "pr_failed_pressure",
            "required_run_pressure",
            "required local checks need the runner lane",
            "runner-pressure.env",
            "pending PR-local checks own the runner lane",
            "dispatching Runner Sustain lane",
            "runner-flow-audit",
            "--strict",
            "runner-black-factor-audit",
            "createdAt,updatedAt,event,displayTitle",
            "--limit 1000",
            "actions/upload-artifact@v7",
        ] {
            assert!(
                workflow.contains(required),
                "black-factor watch missing {required}"
            );
        }
        assert!(target.contains("Runner Black Factor Watch"));
        assert!(target.contains("schedule-driven and event-driven"));
        assert!(target.contains("required-check pressure clears"));
        assert!(target.contains("tops up a small `Runner Sustain` active/queued backlog"));
        assert!(
            target.contains("pending PR or main-branch local checks make the watch record a non-strict audit and stay green")
        );
        assert!(target.contains("clamped to 1-4"));
        assert!(target.contains("defaults to 4"));
        assert!(target.contains("latest 12-hour proof window"));
    }

    #[test]
    fn runner_sustain_workflow_keeps_local_slots_useful() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let workflow = fs::read_to_string(root.join(".github/workflows/runner-sustain.yml"))
            .expect("read runner sustain workflow");
        let target = fs::read_to_string(root.join("docs/forge-loop/kclaw0-runner-flow-target.md"))
            .expect("read kclaw0 runner target");

        for required in [
            "workflow_dispatch:",
            "*/5 * * * *",
            "runs-on: [self-hosted, linux, x64, local, flexnetos]",
            "lane_slot",
            "Runner sustain slot ${{ inputs.lane_slot || '1' }}",
            "pull-requests: read",
            "actions: write",
            "forge-loop components-audit --strict",
            "forge-loop target-mining-audit --strict",
            "forge-loop docs-drift --json",
            "Refill Runner Sustain backlog on completion",
            "MIN_SUSTAIN_BACKLOG: '4'",
            "dispatching Runner Sustain self-refill lane",
            "dispatching Runner Black Factor Watch sustain-completion wakeup",
            "gh workflow run runner-black-factor-watch.yml --ref main -f trigger_source=sustain_completion",
            "skipping Runner Sustain self-refill because",
            "gh workflow run runner-sustain.yml --ref main",
        ] {
            assert!(
                workflow.contains(required),
                "sustain workflow missing {required}"
            );
        }
        for required in [
            "300-agent",
            "4000-step",
            "12+ hour",
            "24/7 autonomous",
            "up to four active/queued one-lane `Runner Sustain` workflow runs",
            "self-refill replacement",
            "duration-proven workflow-run opportunities",
            "every 30 seconds",
            "preserving seamless PR flow",
        ] {
            assert!(
                target.contains(required),
                "runner target missing {required}"
            );
        }
    }

    #[test]
    fn ci_runs_docs_drift_guard() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let ci =
            fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("read CI workflow");
        assert!(
            ci.contains("forge-loop docs-drift"),
            "CI must run the forge-loop docs drift guard"
        );
    }

    #[test]
    fn docs_drift_guard_flags_exported_feature_still_queued() {
        let root = std::env::temp_dir().join(format!(
            "fxrun-docs-drift-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(root.join("docs")).expect("docs dir");
        fs::create_dir_all(root.join("crates/runner-core/src")).expect("src dir");
        fs::write(root.join("crates/runner-core/src/stategate.rs"), "").expect("module");
        fs::write(
            root.join("docs/kclaw0-upgrade-ledger.md"),
            "- ▷ **State-gated route admission** — Queued after PR #31.\n",
        )
        .expect("ledger");

        let report = docs_drift_report(&root).expect("report");
        assert_eq!(report.drift.len(), 1);
        assert!(report.drift[0].contains("State-gated route admission"));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn docs_drift_guard_flags_exported_feature_still_in_backlog_tier() {
        let root = std::env::temp_dir().join(format!(
            "fxrun-docs-drift-tier-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(root.join("docs")).expect("docs dir");
        fs::create_dir_all(root.join("crates/runner-core/src")).expect("src dir");
        fs::write(root.join("crates/runner-core/src/events.rs"), "").expect("module");
        fs::write(
            root.join("docs/kclaw0-upgrade-ledger.md"),
            "## Applied\n\
             | Runner upgrade | Where |\n\
             |---|---|\n\
             | **Rule-citation audit schema** | `runner-core::events` |\n\
             \n\
             ### Tier 1 — automation and orchestration expansion\n\
             11. **Rule-citation audit schema** — every policy refusal carries denial metadata.\n\
             ",
        )
        .expect("ledger");

        let report = docs_drift_report(&root).expect("report");
        assert_eq!(report.drift.len(), 1);
        assert!(report.drift[0].contains("Rule-citation audit schema"));

        fs::remove_dir_all(root).ok();
    }
}
