use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_CODEX: &str = "/home/flexnetos/.local/bin/codex";
const DEFAULT_CODEX_HOME: &str = "/home/flexnetos/.codex";
const DEFAULT_ARTIFACT_ROOT: &str = "_work/forge-loop";
const MAX_EVAL_RETRY_COUNT: u8 = 10;
const REQUIRED_LOCAL_CHECKS: &[&str] = &["Local Linux CI", "Semantic PR Title"];
const REQUIRED_CHECK_WORKFLOWS: &[&str] = &["ci.yml", "semantic-pr-title.yml"];
const LOCAL_FLEXNETOS_RUNNER_LABELS: &[&str] =
    &["self-hosted", "linux", "x64", "local", "flexnetos"];
const LOCAL_FLEXNETOS_RUNNER_ROLE: &str = "execute GitHub Actions jobs that request the shared FlexNetOS local self-hosted label set: self-hosted, linux, x64, local, flexnetos";
const RUNNER_QUEUE_CONTROLLER_ROLE: &str = "collect active/queued job evidence across repos, separate local-runner pressure from GitHub-hosted or vendor queues, and fix/dispatch/yield work so PR checks and repo sessions do not wait blindly";
const SEMANTIC_PR_TITLE_INPUT: &str = "pr_title";
const CYCLE_MANIFEST_SCHEMA_VERSION: u8 = 1;
const AUTO_COMPACT_TOKEN_LIMIT: u32 = 3_000_000;
const TOOL_OUTPUT_TOKEN_LIMIT: u32 = 12_000;
const COMPACT_PROMPT_PATH: &str = ".codex/prompts/compact-forge-loop.md";
const CODEX_OUTPUT_SCHEMA_PATH: &str = ".github/codex/schemas/forge-loop-output.schema.json";
const CODEX_FORGE_LOOP_OUTPUT: &str = "codex-forge-loop-output.md";
const REQUIRED_GATE_COMMANDS: &[&str] = &[
    "rtk cargo fmt --all -- --check",
    "rtk cargo test -p runner-cli --all-features forge_loop::tests",
    "rtk cargo run -q -p runner-cli -- forge-loop doctor --json",
    "rtk cargo run -q -p runner-cli -- forge-loop docs-drift --json",
    "rtk cargo run -q -p runner-cli -- forge-loop components-audit --strict",
    "rtk cargo run -q -p runner-cli -- forge-loop target-mining-audit --strict",
    "rtk cargo run -q -p runner-cli -- forge-loop research --dry-run --focus \"reliability, accuracy, and speed\"",
    "rtk cargo run -q -p runner-cli -- forge-loop output-schema-audit --strict",
    "rtk cargo run -q -p runner-cli -- forge-loop run --dry-run --out /tmp/fxrun-forge-loop-gate-dry-run --goal \"scheduled subscription-auth Codex self-improvement\"",
    "rtk cargo run -q -p runner-cli -- forge-loop eval --fixture",
    "rtk cargo run -q -p runner-cli -- forge-loop eval --metrics /tmp/fxrun-forge-loop-gate-dry-run/cycle/evaluation-input.json --manifest /tmp/fxrun-forge-loop-gate-dry-run/cycle/cycle-manifest.json",
    "rtk cargo run -q -p runner-cli -- forge-loop self-upgrade --dry-run",
    "rtk cargo run -q -p runner-cli -- forge-loop runner-flow-audit --json",
    "rtk cargo run -q -p runner-cli -- forge-loop agentic-system-audit --json",
    "rtk cargo test --workspace --all-features",
    "rtk cargo clippy --workspace --all-targets --all-features -- -D warnings",
    "rtk cargo audit --deny warnings",
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
    /// Audit queued/in-progress jobs across repos that are waiting on the shared local runner labels.
    RunnerQueueAudit(RunnerQueueAuditArgs),
    /// Audit the full 24/7 agentic loop: research, evaluation, adaptation, growth, runners, and PR flow.
    AgenticSystemAudit(AgenticSystemAuditArgs),
    /// Fail when exported forge-loop upgrades are still documented as queued/backlog work.
    DocsDrift(DocsDriftArgs),
    /// Inventory Codex loop components and config surfaces for upgrade planning.
    ComponentsAudit(ComponentsAuditArgs),
    /// Verify required Codex target mining sources were extracted, applied, and guarded.
    TargetMiningAudit(TargetMiningAuditArgs),
    /// Verify the structured Codex output schema still requires critical loop evidence.
    OutputSchemaAudit(OutputSchemaAuditArgs),
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
    /// JSON from `gh pr list --state all --json number,title,state,mergedAt,statusCheckRollup,url`.
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
    /// Optional JSON from `gh pr list --state all --json number,title,state,mergedAt,statusCheckRollup,url` used to prove failed Codex growth was recovered by a clean merged self-upgrade PR.
    #[arg(long)]
    pub prs_history_json: Option<PathBuf>,
    /// Minimum observed burn-in window in hours.
    #[arg(long, default_value_t = 1)]
    pub min_window_hours: u64,
    /// Maximum allowed observed idle gap between useful Runner Sustain intervals.
    #[arg(long, default_value_t = 10)]
    pub max_idle_gap_minutes: u64,
    /// Minimum active/queued Runner Sustain backlog required at audit time unless Codex growth is already active or queued.
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
    /// Expected repository or repository-prefix scope that may own local dark-factory lanes. A value ending in `/` matches every repo in that owner/org.
    #[arg(
        long = "expected-scope",
        alias = "expected-repository",
        default_value = "FlexNetOS/"
    )]
    pub expected_scope: String,
    /// Optional JSON fixture/input of observed GitHub Actions jobs; when omitted, scan /proc.
    #[arg(long)]
    pub jobs_json: Option<PathBuf>,
    /// procfs root to scan for live GitHub Actions job environments.
    #[arg(long, default_value = "/proc")]
    pub proc_root: PathBuf,
    /// Maximum out-of-scope repository jobs allowed to occupy local runner lanes.
    #[arg(
        long = "max-out-of-scope-jobs",
        alias = "max-external-jobs",
        default_value_t = 0
    )]
    pub max_out_of_scope_jobs: usize,
    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,
    /// Return non-zero when out-of-scope lane pressure exceeds the allowed budget.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Args, Debug, Clone)]
pub struct RunnerQueueAuditArgs {
    /// Combined JSON array of repository run snapshots with each run's GitHub Actions jobs.
    #[arg(long)]
    pub repo_jobs_json: PathBuf,
    /// Maximum queued local-label jobs allowed before strict mode reports local runner pressure.
    #[arg(long, default_value_t = 0)]
    pub max_queued_local_jobs: usize,
    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,
    /// Return non-zero when local-label queue pressure exceeds the allowed budget.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Args, Debug, Clone)]
pub struct AgenticSystemAuditArgs {
    /// Workspace root to scan.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// JSON from `gh run list --limit 3000 --json name,status,conclusion,createdAt,updatedAt,event,displayTitle,url`.
    #[arg(long)]
    pub runs_json: Option<PathBuf>,
    /// JSON from `gh pr list --state open --json number,title,state,mergedAt,statusCheckRollup,url`.
    #[arg(long)]
    pub open_prs_json: Option<PathBuf>,
    /// JSON from `gh pr list --state all --json number,title,state,mergedAt,statusCheckRollup,url`.
    #[arg(long)]
    pub prs_history_json: Option<PathBuf>,
    /// Expected repository or repository-prefix scope that may own local dark-factory lanes. A value ending in `/` matches every repo in that owner/org.
    #[arg(
        long = "expected-scope",
        alias = "expected-repository",
        default_value = "FlexNetOS/"
    )]
    pub expected_scope: String,
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
    /// Minimum active/queued Runner Sustain backlog required at audit time unless Codex growth is already active or queued.
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
    /// Maximum out-of-scope repository jobs allowed to occupy local runner lanes.
    #[arg(
        long = "max-out-of-scope-jobs",
        alias = "max-external-jobs",
        default_value_t = 0
    )]
    pub max_out_of_scope_jobs: usize,
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

#[derive(Args, Debug, Clone)]
pub struct OutputSchemaAuditArgs {
    /// Workspace root to scan.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,
    /// Return a non-zero exit when the schema omits required evidence.
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
    #[serde(default)]
    pub cycle_goal: Option<String>,
    #[serde(default)]
    pub prompt_sha256: Option<String>,
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
pub struct CodexAuthReadiness {
    pub auth_mode: &'static str,
    pub codex_home: String,
    pub auth_json: String,
    pub auth_json_present: bool,
    pub login_status_checked: bool,
    pub login_status_command: &'static str,
    pub verification_commands: Vec<String>,
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
    pub max_unrecovered_idle_gap_minutes: u64,
    pub recovered_idle_gap_minutes: u64,
    pub recovered_idle_gaps: usize,
    pub max_idle_gap_minutes: u64,
    pub active_or_queued_sustain_runs: usize,
    pub active_or_queued_codex_growth_runs: usize,
    pub sustain_or_growth_backlog_ready: bool,
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
    pub expected_scope: String,
    pub total_jobs: usize,
    pub in_scope_repository_jobs: usize,
    pub out_of_scope_repository_jobs: usize,
    pub max_out_of_scope_jobs: usize,
    pub out_of_scope_repositories: BTreeMap<String, usize>,
    pub jobs: Vec<RunnerFleetJob>,
    pub fleet_ready: bool,
    pub missing_evidence: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnerQueueJobSummary {
    pub repository: String,
    pub workflow: String,
    pub run_id: String,
    pub run_status: String,
    pub event: String,
    pub display_title: String,
    pub head_branch: String,
    pub job: String,
    pub status: String,
    pub conclusion: String,
    pub runner_name: String,
    pub runner_group_name: String,
    pub labels: Vec<String>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnerQueueRepositorySummary {
    pub repository: String,
    pub active_local_runner_jobs: usize,
    pub queued_local_runner_jobs: usize,
    pub nonlocal_queued_jobs: usize,
    pub trigger_events: BTreeMap<String, usize>,
    pub workflows: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnerQueueAuditReport {
    pub kclaw0_target: &'static str,
    pub local_runner_labels: Vec<String>,
    pub runner_role: &'static str,
    pub controller_role: &'static str,
    pub scanned_repositories: usize,
    pub scanned_runs: usize,
    pub scanned_jobs: usize,
    pub active_local_runner_jobs: Vec<RunnerQueueJobSummary>,
    pub queued_local_runner_jobs: Vec<RunnerQueueJobSummary>,
    pub nonlocal_queued_jobs: Vec<RunnerQueueJobSummary>,
    pub repositories: Vec<RunnerQueueRepositorySummary>,
    pub local_runner_busy_repositories: BTreeMap<String, usize>,
    pub local_runner_waiting_repositories: BTreeMap<String, usize>,
    pub trigger_events: BTreeMap<String, usize>,
    pub max_queued_local_jobs: usize,
    pub queue_ready: bool,
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
    #[serde(default)]
    title: String,
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
struct RunnerQueueRunInput {
    #[serde(default)]
    repository: String,
    #[serde(default, alias = "runId", alias = "databaseId")]
    #[serde(deserialize_with = "deserialize_stringish")]
    run_id: String,
    #[serde(default, alias = "name", alias = "runName", alias = "run_name")]
    workflow: String,
    #[serde(default, alias = "runStatus", alias = "run_status")]
    run_status: String,
    #[serde(default)]
    event: String,
    #[serde(default, alias = "displayTitle", alias = "display_title")]
    display_title: String,
    #[serde(default, alias = "headBranch", alias = "head_branch")]
    head_branch: String,
    #[serde(default, alias = "html_url", alias = "htmlUrl")]
    url: String,
    #[serde(default)]
    jobs: Vec<RunnerQueueJobInput>,
}

#[derive(Debug, Clone, Deserialize)]
struct RunnerQueueJobInput {
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_stringish")]
    conclusion: String,
    #[serde(default, alias = "runnerName", alias = "runner_name")]
    runner_name: String,
    #[serde(default, alias = "runnerGroupName", alias = "runner_group_name")]
    runner_group_name: String,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default, alias = "html_url", alias = "htmlUrl")]
    url: String,
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
    pub permission_profile_readiness: PermissionProfileReadiness,
    pub checklist_shell_discipline: ChecklistShellDisciplineReadiness,
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
pub struct PermissionProfileReadiness {
    pub active_default_permissions: Option<String>,
    pub active_sandbox_mode: Option<String>,
    pub mirror_default_permissions: Option<String>,
    pub profile_rules_present: bool,
    pub migration_ready: bool,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChecklistShellDisciplineReadiness {
    pub checklist_path: &'static str,
    pub checked_commands: Vec<String>,
    pub raw_command_keys: Vec<String>,
    pub rtk_ready: bool,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetMiningAuditReport {
    pub checked_targets: usize,
    pub covered_targets: Vec<String>,
    pub missing_targets: Vec<String>,
    pub targets: Vec<TargetMiningStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputSchemaAuditReport {
    pub schema_path: &'static str,
    pub required_fields: Vec<String>,
    pub present_fields: Vec<String>,
    pub missing_fields: Vec<String>,
    pub schema_valid_json: bool,
    pub structured_output_ready: bool,
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
    pub schema_version: u8,
    pub goal: String,
    pub pr_title: String,
    pub prompt_sha256: String,
    pub once: bool,
    pub auto_merge: bool,
    pub strict_upgrade_only: bool,
    pub phases: Vec<CyclePhase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactContinuityArtifact {
    pub enabled: bool,
    pub compact_prompt: String,
    pub compact_summary_events: Vec<String>,
    pub phases: Vec<CyclePhase>,
    pub active_phase: CyclePhase,
    pub current_phase_index: usize,
    pub source_coverage: Vec<String>,
    pub research_output_contract: Vec<String>,
    pub validation_state: Vec<String>,
    pub validation_terminal_state: Vec<String>,
    pub validation_sources: Vec<String>,
    pub phase_continuity: Vec<String>,
    pub phase_next_actions: BTreeMap<String, String>,
    pub phase_validation_commands: BTreeMap<String, Vec<String>>,
    pub phase_validation_state: BTreeMap<String, String>,
    pub next_action: String,
    pub phase_source_validation_next_action: String,
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
        ForgeLoopCommand::RunnerQueueAudit(args) => runner_queue_audit(args),
        ForgeLoopCommand::AgenticSystemAudit(args) => agentic_system_audit(args),
        ForgeLoopCommand::DocsDrift(args) => docs_drift(args),
        ForgeLoopCommand::ComponentsAudit(args) => components_audit(args),
        ForgeLoopCommand::TargetMiningAudit(args) => target_mining_audit(args),
        ForgeLoopCommand::OutputSchemaAudit(args) => output_schema_audit(args),
    }
}

fn run(args: RunArgs) -> Result<()> {
    if !args.once {
        return Err(anyhow!(
            "once must be true; forge-loop run executes exactly one supervised cycle"
        ));
    }

    let cycle_dir = if args.dry_run {
        args.out.join("cycle")
    } else {
        args.out.join(timestamp_label()?)
    };
    fs::create_dir_all(&cycle_dir)
        .with_context(|| format!("create forge-loop artifact dir {}", cycle_dir.display()))?;
    let manifest = cycle_manifest(&args);
    fs::write(
        cycle_dir.join("cycle-manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    fs::write(
        cycle_dir.join("research-sources.json"),
        serde_json::to_string_pretty(&research_sources())?,
    )?;
    fs::write(
        cycle_dir.join("codex-auth-readiness.json"),
        serde_json::to_string_pretty(&codex_auth_readiness())?,
    )?;
    fs::write(
        cycle_dir.join("required-gates.json"),
        serde_json::to_string_pretty(&REQUIRED_GATE_COMMANDS)?,
    )?;
    let compact_continuity = compact_continuity_artifact();
    fs::write(
        cycle_dir.join("compact-continuity.json"),
        serde_json::to_string_pretty(&compact_continuity)?,
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
    append_event(
        &log,
        CycleEvent {
            event: "continuity.compact_checkpoint",
            phase: compact_continuity.active_phase,
            detail: &compact_continuity.phase_source_validation_next_action,
        },
    )?;

    let prompt = cycle_prompt(&args.goal, args.auto_merge);
    let invocation = codex_invocation(prompt);
    fs::write(
        cycle_dir.join("codex-invocation.json"),
        serde_json::to_string_pretty(&invocation)?,
    )?;

    if args.dry_run {
        let eval_input = EvalInput::fixture();
        fs::write(
            cycle_dir.join("evaluation-input.json"),
            serde_json::to_string_pretty(&eval_input)?,
        )?;
        append_event(
            &log,
            CycleEvent {
                event: "cycle.dry_run",
                phase: CyclePhase::Implement,
                detail: "codex invocation planned but not executed",
            },
        )?;
        let report = evaluate(eval_input);
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

    let pr_title = cycle_pr_title(&args.goal);
    match publish_self_upgrade_if_needed(&pr_title, args.auto_merge, &log)? {
        Some(pr_url) => append_event(
            &log,
            CycleEvent {
                event: "publish.pr_opened",
                phase: CyclePhase::Upgrade,
                detail: &pr_url,
            },
        )?,
        None => append_event(
            &log,
            CycleEvent {
                event: "publish.no_changes",
                phase: CyclePhase::Evaluate,
                detail: "codex completed without publishable repository changes",
            },
        )?,
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
        let manifest = parse_cycle_manifest(&path)?;
        validate_eval_manifest_pair(&input, &manifest)
            .with_context(|| format!("validate metrics against manifest {}", path.display()))?;
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
    let plan = self_upgrade_plan(args.min_score);
    println!("{}", serde_json::to_string_pretty(&plan)?);
    if args.dry_run || !allowed {
        return Ok(());
    }
    let prompt = self_upgrade_prompt(report.score);
    let invocation = codex_invocation(prompt);
    let status = Command::new(&invocation.program)
        .args(&invocation.args)
        .status()?;
    if !status.success() {
        return Err(anyhow!("codex self-upgrade failed with status {status}"));
    }
    Ok(())
}

fn self_upgrade_plan(min_score: u8) -> serde_json::Value {
    let report = evaluate(EvalInput::fixture());
    let allowed = report.score >= min_score && report.upgrade_allowed;
    serde_json::json!({
        "score": report.score,
        "min_score": min_score,
        "allowed": allowed,
        "branch_prefix": "codex/forge-loop-self-upgrade",
        "merge_policy": "auto-merge green when repository settings allow; otherwise merge after green checks",
        "strict_upgrade_only": true,
        "runner_health_input": "gh pr view <PR> --json statusCheckRollup",
        "required_local_checks": REQUIRED_LOCAL_CHECKS,
        "required_gate_commands": REQUIRED_GATE_COMMANDS,
        "components_audit": "rtk fxrun forge-loop components-audit --json",
        "target_mining_audit": "rtk fxrun forge-loop target-mining-audit --json",
        "runner_flow_audit": "rtk fxrun forge-loop runner-flow-audit --json",
        "runner_black_factor_audit": "rtk fxrun forge-loop runner-black-factor-audit --json",
        "runner_ops_slo_audit": "rtk fxrun forge-loop runner-ops-slo-audit --json",
        "runner_fleet_audit": "rtk fxrun forge-loop runner-fleet-audit --json",
        "runner_queue_audit": "rtk fxrun forge-loop runner-queue-audit --repo-jobs-json <repo-jobs.json> --json",
        "agentic_system_audit": "rtk fxrun forge-loop agentic-system-audit --json",
        "compact_continuity": "compact-continuity.json",
        "phase_next_actions": phase_next_actions(),
        "phase_validation_commands": phase_validation_commands(),
        "phase_validation_state": phase_validation_state()
    })
}

fn self_upgrade_prompt(score: u8) -> String {
    format!(
        "You are the forge-loop self-upgrade agent. Implement exactly one small, TDD-first reliability, accuracy, or speed improvement for fxrun forge-loop. Leave the intended repository changes in the working tree; do not run git commit, git push, or gh pr from inside Codex. The outer forge-loop engine will commit, push, open a PR, and enable auto-merge if checks are green when repository settings allow. Evaluation score: {score}. Strict upgrade only; no downgrades/removals without parity proof. Shell discipline: prefix every shell command with `rtk`; for Unix `find` with compound predicates or actions, use `rtk proxy find ...` instead of `rtk find ...` because `rtk find` rejects compound predicates."
    )
}

fn doctor(args: DoctorArgs) -> Result<()> {
    let report = serde_json::json!({
        "codex": codex_program(),
        "codex_auth": codex_auth_readiness(),
        "artifact_root": DEFAULT_ARTIFACT_ROOT,
        "research_sources": research_sources(),
        "phases": ["red", "implement", "gate", "evaluate", "research", "upgrade"],
        "auto_merge_green": true,
        "strict_upgrade_only": true,
        "runner_health_input": "gh pr view <PR> --json statusCheckRollup",
        "required_local_checks": REQUIRED_LOCAL_CHECKS,
        "required_gate_commands": REQUIRED_GATE_COMMANDS,
        "target_mining_audit": "rtk fxrun forge-loop target-mining-audit --json",
        "runner_flow_audit": "rtk fxrun forge-loop runner-flow-audit --json",
        "runner_black_factor_audit": "rtk fxrun forge-loop runner-black-factor-audit --json",
        "runner_ops_slo_audit": "rtk fxrun forge-loop runner-ops-slo-audit --json",
        "runner_fleet_audit": "rtk fxrun forge-loop runner-fleet-audit --json",
        "runner_queue_audit": "rtk fxrun forge-loop runner-queue-audit --repo-jobs-json <repo-jobs.json> --json",
        "agentic_system_audit": "rtk fxrun forge-loop agentic-system-audit --json"
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("fxrun forge-loop");
        println!("  codex cli          : {}", codex_program());
        let auth = codex_auth_readiness();
        println!("  codex auth mode    : {}", auth.auth_mode);
        println!("  codex auth file    : {}", auth.auth_json);
        println!("  auth file present  : {}", auth.auth_json_present);
        println!("  auth status check  : {}", auth.login_status_command);
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
        println!("  queue role audit   : use `fxrun forge-loop runner-queue-audit --repo-jobs-json <repo-jobs.json> --json`");
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
        println!(
            "  unrecovered idle min: {}",
            report.max_unrecovered_idle_gap_minutes
        );
        println!(
            "  recovered idle min  : {}",
            report.recovered_idle_gap_minutes
        );
        println!("  recovered idle gaps : {}", report.recovered_idle_gaps);
        println!("  allowed idle gap min : {}", report.max_idle_gap_minutes);
        println!(
            "  active/queued sustain: {}",
            report.active_or_queued_sustain_runs
        );
        println!(
            "  active/queued codex  : {}",
            report.active_or_queued_codex_growth_runs
        );
        println!(
            "  sustain/growth ready : {}",
            report.sustain_or_growth_backlog_ready
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
        println!("  expected scope      : {}", report.expected_scope);
        println!("  total jobs          : {}", report.total_jobs);
        println!("  in-scope repo jobs : {}", report.in_scope_repository_jobs);
        println!(
            "  out-of-scope jobs  : {}",
            report.out_of_scope_repository_jobs
        );
        println!("  max out-of-scope   : {}", report.max_out_of_scope_jobs);
        if !report.out_of_scope_repositories.is_empty() {
            println!("  out-of-scope repos:");
            for (repo, count) in &report.out_of_scope_repositories {
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

fn runner_queue_audit(args: RunnerQueueAuditArgs) -> Result<()> {
    let report = runner_queue_audit_report(&args)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("fxrun forge-loop runner queue audit");
        println!("  kclaw0 target        : {}", report.kclaw0_target);
        println!(
            "  local labels         : {}",
            report.local_runner_labels.join(", ")
        );
        println!("  runner role          : {}", report.runner_role);
        println!("  controller role      : {}", report.controller_role);
        println!("  scanned repositories : {}", report.scanned_repositories);
        println!("  scanned runs         : {}", report.scanned_runs);
        println!("  scanned jobs         : {}", report.scanned_jobs);
        println!(
            "  active local jobs    : {}",
            report.active_local_runner_jobs.len()
        );
        println!(
            "  queued local jobs    : {}",
            report.queued_local_runner_jobs.len()
        );
        println!(
            "  nonlocal queued jobs : {}",
            report.nonlocal_queued_jobs.len()
        );
        if !report.local_runner_busy_repositories.is_empty() {
            println!("  local lanes busy:");
            for (repo, count) in &report.local_runner_busy_repositories {
                println!("    - {repo}: {count}");
            }
        }
        if !report.local_runner_waiting_repositories.is_empty() {
            println!("  local-label queues:");
            for (repo, count) in &report.local_runner_waiting_repositories {
                println!("    - {repo}: {count}");
            }
        }
        if !report.trigger_events.is_empty() {
            println!("  trigger events:");
            for (event, count) in &report.trigger_events {
                println!("    - {event}: {count}");
            }
        }
        println!("  queue ready          : {}", report.queue_ready);
        if !report.missing_evidence.is_empty() {
            println!("  missing evidence     :");
            for item in &report.missing_evidence {
                println!("    - {item}");
            }
        }
    }

    if args.strict && !report.queue_ready {
        Err(anyhow!(
            "runner queue audit evidence incomplete: {}",
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
    let pr_history = match &args.prs_history_json {
        Some(path) => parse_json_vec::<PrHistoryEntry>(path)?,
        None => Vec::new(),
    };

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
    let active_or_queued_codex_growth_runs = runs
        .iter()
        .filter(|run| is_codex_forge_loop_name(&run.name))
        .filter(|run| {
            run.status.eq_ignore_ascii_case("queued")
                || run.status.eq_ignore_ascii_case("pending")
                || run.status.eq_ignore_ascii_case("in_progress")
        })
        .count();
    let sustain_or_growth_backlog_ready = active_or_queued_sustain_runs
        >= args.min_active_or_queued_sustain
        || active_or_queued_codex_growth_runs > 0;

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
        .filter(|run| is_failed_ops_run(run, &runs, &pr_history))
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

    let idle_gap_recovery = local_runner_idle_gap_recovery(
        &runs,
        &pr_history,
        proof_window_start,
        observed_end,
        args.min_sustain_duration_minutes,
    );

    let mut missing_evidence = Vec::new();
    if observed_window_minutes < min_window_minutes {
        missing_evidence.push("observed_slo_window");
    }
    if idle_gap_recovery.max_unrecovered_idle_gap_minutes > args.max_idle_gap_minutes {
        missing_evidence.push("idle_gap_slo");
    }
    if !sustain_or_growth_backlog_ready {
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
        kclaw0_target: "unattended dark-factory operations burn-in with bounded idle gaps, event-driven rehydration, clean PR flow, and zero unrecovered failed operational runs",
        observed_window_minutes,
        min_window_minutes,
        max_idle_gap_minutes_observed: idle_gap_recovery.max_idle_gap_minutes_observed,
        max_unrecovered_idle_gap_minutes: idle_gap_recovery.max_unrecovered_idle_gap_minutes,
        recovered_idle_gap_minutes: idle_gap_recovery.recovered_idle_gap_minutes,
        recovered_idle_gaps: idle_gap_recovery.recovered_idle_gaps,
        max_idle_gap_minutes: args.max_idle_gap_minutes,
        active_or_queued_sustain_runs,
        active_or_queued_codex_growth_runs,
        sustain_or_growth_backlog_ready,
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
    let in_scope_repository_jobs = jobs
        .iter()
        .filter(|job| repository_matches_expected_scope(&job.repository, &args.expected_scope))
        .count();
    let mut out_of_scope_repositories = BTreeMap::new();
    for job in jobs
        .iter()
        .filter(|job| !repository_matches_expected_scope(&job.repository, &args.expected_scope))
    {
        *out_of_scope_repositories
            .entry(job.repository.clone())
            .or_insert(0) += 1;
    }
    let out_of_scope_repository_jobs = jobs.len().saturating_sub(in_scope_repository_jobs);
    let mut missing_evidence = Vec::new();
    if out_of_scope_repository_jobs > args.max_out_of_scope_jobs {
        missing_evidence.push("out_of_scope_runner_lane_pressure");
    }
    let fleet_ready = missing_evidence.is_empty();

    Ok(RunnerFleetAuditReport {
        kclaw0_target: "local self-hosted runner lanes are attributable across the full FlexNetOS org by default; out-of-scope ownership is explicit instead of hiding behind a single-repo proof",
        expected_scope: args.expected_scope.clone(),
        total_jobs: jobs.len(),
        in_scope_repository_jobs,
        out_of_scope_repository_jobs,
        max_out_of_scope_jobs: args.max_out_of_scope_jobs,
        out_of_scope_repositories,
        jobs,
        fleet_ready,
        missing_evidence,
    })
}

fn runner_queue_audit_report(args: &RunnerQueueAuditArgs) -> Result<RunnerQueueAuditReport> {
    let runs =
        dedupe_runner_queue_runs(parse_json_vec::<RunnerQueueRunInput>(&args.repo_jobs_json)?);
    let mut jobs = Vec::new();
    let mut repository_runs: BTreeMap<String, usize> = BTreeMap::new();
    let mut repository_events: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut repository_workflows: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut trigger_events: BTreeMap<String, usize> = BTreeMap::new();

    for run in &runs {
        let repository = run.repository.clone();
        *repository_runs.entry(repository.clone()).or_insert(0) += 1;
        if !run.event.is_empty() {
            *trigger_events.entry(run.event.clone()).or_insert(0) += 1;
            *repository_events
                .entry(repository.clone())
                .or_default()
                .entry(run.event.clone())
                .or_insert(0) += 1;
        }
        if !run.workflow.is_empty() {
            *repository_workflows
                .entry(repository.clone())
                .or_default()
                .entry(run.workflow.clone())
                .or_insert(0) += 1;
        }
        for job in &run.jobs {
            jobs.push(RunnerQueueJobSummary {
                repository: repository.clone(),
                workflow: run.workflow.clone(),
                run_id: run.run_id.clone(),
                run_status: run.run_status.clone(),
                event: run.event.clone(),
                display_title: run.display_title.clone(),
                head_branch: run.head_branch.clone(),
                job: job.name.clone(),
                status: job.status.clone(),
                conclusion: job.conclusion.clone(),
                runner_name: job.runner_name.clone(),
                runner_group_name: job.runner_group_name.clone(),
                labels: job.labels.clone(),
                url: if job.url.is_empty() {
                    run.url.clone()
                } else {
                    job.url.clone()
                },
            });
        }
    }

    let active_local_runner_jobs = jobs
        .iter()
        .filter(|job| job_has_local_flexnetos_labels(&job.labels))
        .filter(|job| is_active_runner_job_status(&job.status))
        .cloned()
        .collect::<Vec<_>>();
    let queued_local_runner_jobs = jobs
        .iter()
        .filter(|job| job_has_local_flexnetos_labels(&job.labels))
        .filter(|job| is_queued_runner_job_status(&job.status))
        .cloned()
        .collect::<Vec<_>>();
    let nonlocal_queued_jobs = jobs
        .iter()
        .filter(|job| !job_has_local_flexnetos_labels(&job.labels))
        .filter(|job| is_queued_runner_job_status(&job.status))
        .cloned()
        .collect::<Vec<_>>();

    let local_runner_busy_repositories = count_jobs_by_repository(&active_local_runner_jobs);
    let local_runner_waiting_repositories = count_jobs_by_repository(&queued_local_runner_jobs);

    let mut repositories = repository_runs
        .keys()
        .map(|repository| RunnerQueueRepositorySummary {
            repository: repository.clone(),
            active_local_runner_jobs: local_runner_busy_repositories
                .get(repository)
                .copied()
                .unwrap_or(0),
            queued_local_runner_jobs: local_runner_waiting_repositories
                .get(repository)
                .copied()
                .unwrap_or(0),
            nonlocal_queued_jobs: nonlocal_queued_jobs
                .iter()
                .filter(|job| &job.repository == repository)
                .count(),
            trigger_events: repository_events
                .get(repository)
                .cloned()
                .unwrap_or_default(),
            workflows: repository_workflows
                .get(repository)
                .cloned()
                .unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    repositories.sort_by(|left, right| left.repository.cmp(&right.repository));

    let mut missing_evidence = Vec::new();
    if runs.is_empty() {
        missing_evidence.push("repo_job_snapshots");
    }
    if queued_local_runner_jobs.len() > args.max_queued_local_jobs {
        missing_evidence.push("local_runner_queue_pressure");
    }
    let queue_ready = missing_evidence.is_empty();

    Ok(RunnerQueueAuditReport {
        kclaw0_target: "shared local runner queue is explicit: active lanes, waiting local-label jobs, nonlocal vendor queues, repo triggers, and controller responsibility are machine-classified",
        local_runner_labels: LOCAL_FLEXNETOS_RUNNER_LABELS
            .iter()
            .map(|label| (*label).to_string())
            .collect(),
        runner_role: LOCAL_FLEXNETOS_RUNNER_ROLE,
        controller_role: RUNNER_QUEUE_CONTROLLER_ROLE,
        scanned_repositories: repository_runs.len(),
        scanned_runs: runs.len(),
        scanned_jobs: jobs.len(),
        active_local_runner_jobs,
        queued_local_runner_jobs,
        nonlocal_queued_jobs,
        repositories,
        local_runner_busy_repositories,
        local_runner_waiting_repositories,
        trigger_events,
        max_queued_local_jobs: args.max_queued_local_jobs,
        queue_ready,
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
                prs_history_json: args.prs_history_json.clone(),
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
        expected_scope: args.expected_scope.clone(),
        jobs_json: args.fleet_jobs_json.clone(),
        proc_root: args.proc_root.clone(),
        max_out_of_scope_jobs: args.max_out_of_scope_jobs,
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
                "local ChatGPT auth",
            ),
            (".github/workflows/codex-forge-loop.yml", "CODEX_HOME"),
            (".github/workflows/codex-forge-loop.yml", "forge-loop run"),
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

fn dedupe_runner_queue_runs(runs: Vec<RunnerQueueRunInput>) -> Vec<RunnerQueueRunInput> {
    let mut by_key: BTreeMap<(String, String), RunnerQueueRunInput> = BTreeMap::new();
    for run in runs {
        let key = (run.repository.clone(), run.run_id.clone());
        by_key
            .entry(key)
            .and_modify(|existing| {
                if existing.workflow.is_empty() {
                    existing.workflow = run.workflow.clone();
                }
                if existing.run_status.is_empty() {
                    existing.run_status = run.run_status.clone();
                }
                if existing.event.is_empty() {
                    existing.event = run.event.clone();
                }
                if existing.display_title.is_empty() {
                    existing.display_title = run.display_title.clone();
                }
                if existing.head_branch.is_empty() {
                    existing.head_branch = run.head_branch.clone();
                }
                if existing.url.is_empty() {
                    existing.url = run.url.clone();
                }
                existing.jobs.extend(run.jobs.clone());
                dedupe_runner_queue_jobs(&mut existing.jobs);
            })
            .or_insert(run);
    }
    by_key.into_values().collect()
}

fn dedupe_runner_queue_jobs(jobs: &mut Vec<RunnerQueueJobInput>) {
    let mut by_key: BTreeMap<(String, String, String, String), RunnerQueueJobInput> =
        BTreeMap::new();
    for job in jobs.drain(..) {
        let key = (
            job.name.clone(),
            job.status.clone(),
            job.runner_name.clone(),
            job.url.clone(),
        );
        by_key.entry(key).or_insert(job);
    }
    jobs.extend(by_key.into_values());
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

#[derive(Debug, Clone, Copy, Default)]
struct IdleGapRecovery {
    max_idle_gap_minutes_observed: u64,
    max_unrecovered_idle_gap_minutes: u64,
    recovered_idle_gap_minutes: u64,
    recovered_idle_gaps: usize,
}

fn local_runner_idle_gap_recovery(
    runs: &[WorkflowRunEntry],
    pr_history: &[PrHistoryEntry],
    window_start: i64,
    window_end: i64,
    min_duration_minutes: u64,
) -> IdleGapRecovery {
    if window_end <= window_start {
        return IdleGapRecovery::default();
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
    let mut recovery = IdleGapRecovery::default();
    for (start, end) in intervals {
        if start > cursor {
            record_idle_gap(&mut recovery, cursor, start, runs, pr_history);
        }
        if end > cursor {
            cursor = end;
        }
    }
    if window_end > cursor {
        record_idle_gap(&mut recovery, cursor, window_end, runs, pr_history);
    }
    recovery
}

fn record_idle_gap(
    recovery: &mut IdleGapRecovery,
    gap_start: i64,
    gap_end: i64,
    runs: &[WorkflowRunEntry],
    pr_history: &[PrHistoryEntry],
) {
    let minutes = ((gap_end - gap_start).max(0) as u64).div_ceil(60);
    recovery.max_idle_gap_minutes_observed = recovery.max_idle_gap_minutes_observed.max(minutes);
    if idle_gap_has_recovery(gap_end, runs, pr_history) {
        recovery.recovered_idle_gap_minutes = recovery.recovered_idle_gap_minutes.max(minutes);
        recovery.recovered_idle_gaps += 1;
    } else {
        recovery.max_unrecovered_idle_gap_minutes =
            recovery.max_unrecovered_idle_gap_minutes.max(minutes);
    }
}

fn idle_gap_has_recovery(
    gap_end: i64,
    runs: &[WorkflowRunEntry],
    pr_history: &[PrHistoryEntry],
) -> bool {
    pr_history.iter().any(|pr| {
        let Some(merged_at) = clean_idle_recovery_pr_merged_at(pr) else {
            return false;
        };
        merged_at >= gap_end
            && merged_at.saturating_sub(gap_end) <= 90 * 60
            && has_successful_runner_rehydration_after(merged_at, runs)
    })
}

fn clean_idle_recovery_pr_merged_at(pr: &PrHistoryEntry) -> Option<i64> {
    if !pr.state.eq_ignore_ascii_case("MERGED") {
        return None;
    }
    if !is_idle_recovery_pr_title(&pr.title) {
        return None;
    }
    if !classify_runner_health(&pr.status_check_rollup)
        .failed_local_checks
        .is_empty()
    {
        return None;
    }
    pr.merged_at.as_deref().and_then(parse_rfc3339_utc_seconds)
}

fn is_idle_recovery_pr_title(title: &str) -> bool {
    let title = title.to_ascii_lowercase();
    (title.contains("rehydrat") || title.contains("idle") || title.contains("sustain"))
        && (title.contains("runner") || title.contains("codex") || title.contains("completion"))
}

fn has_successful_runner_rehydration_after(merged_at: i64, runs: &[WorkflowRunEntry]) -> bool {
    runs.iter().any(|run| {
        let Some(created_at) = run
            .created_at
            .as_deref()
            .and_then(parse_rfc3339_utc_seconds)
        else {
            return false;
        };
        created_at >= merged_at
            && created_at.saturating_sub(merged_at) <= 60 * 60
            && (is_successful_runner_watch_rehydration(run) || is_successful_runner_sustain(run))
    })
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
    if !matches!(run.name.as_str(), "CI" | "Semantic PR Title")
        && !is_codex_forge_loop_name(&run.name)
    {
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
        || is_codex_forge_loop_name(name)
}

fn is_runner_black_factor_watch_name(name: &str) -> bool {
    name == "Runner Black Factor Watch" || name.starts_with("Runner Black Factor Watch (")
}

fn is_codex_forge_loop_name(name: &str) -> bool {
    name == "Codex Forge Loop"
        || name == ".github/workflows/codex-forge-loop.yml"
        || name.starts_with("Codex Forge Loop (")
}

fn is_agentic_system_watch_name(name: &str) -> bool {
    name == "Agentic System Watch" || name.starts_with("Agentic System Watch (")
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
        && (run.display_title.contains("sustain_completion")
            || run.display_title.contains("codex_completion"))
}

fn is_failed_conclusion(conclusion: &str) -> bool {
    matches!(
        conclusion.to_ascii_lowercase().as_str(),
        "failure" | "timed_out" | "cancelled" | "action_required"
    )
}

fn is_failed_ops_run(
    run: &WorkflowRunEntry,
    runs: &[WorkflowRunEntry],
    pr_history: &[PrHistoryEntry],
) -> bool {
    if !is_failed_conclusion(&run.conclusion) {
        return false;
    }
    if has_nearby_successful_replacement(run, runs) {
        return false;
    }
    if is_codex_forge_loop_name(&run.name) && has_nearby_clean_merged_pr_recovery(run, pr_history) {
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
        same_ops_replacement_family(&run.name, &candidate.name)
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

fn has_nearby_clean_merged_pr_recovery(
    run: &WorkflowRunEntry,
    pr_history: &[PrHistoryEntry],
) -> bool {
    let Some(failed_at) = run
        .updated_at
        .as_deref()
        .or(run.created_at.as_deref())
        .and_then(parse_rfc3339_utc_seconds)
    else {
        return false;
    };
    pr_history.iter().any(|pr| {
        pr.state.eq_ignore_ascii_case("MERGED")
            && classify_runner_health(&pr.status_check_rollup)
                .failed_local_checks
                .is_empty()
            && pr
                .merged_at
                .as_deref()
                .and_then(parse_rfc3339_utc_seconds)
                .is_some_and(|merged_at| {
                    merged_at >= failed_at && merged_at.saturating_sub(failed_at) <= 30 * 60
                })
    })
}

fn same_ops_replacement_family(run_name: &str, candidate_name: &str) -> bool {
    run_name == candidate_name
        || (is_runner_black_factor_watch_name(run_name)
            && is_runner_black_factor_watch_name(candidate_name))
        || (is_codex_forge_loop_name(run_name) && is_codex_forge_loop_name(candidate_name))
        || (is_agentic_system_watch_name(run_name) && is_agentic_system_watch_name(candidate_name))
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

fn deserialize_stringish<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(text) => text,
        serde_json::Value::Number(number) => number.to_string(),
        serde_json::Value::Bool(flag) => flag.to_string(),
        other => other.to_string(),
    })
}

fn job_has_local_flexnetos_labels(labels: &[String]) -> bool {
    LOCAL_FLEXNETOS_RUNNER_LABELS.iter().all(|required| {
        labels
            .iter()
            .any(|label| label.eq_ignore_ascii_case(required))
    })
}

fn is_active_runner_job_status(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "in_progress" | "running"
    )
}

fn is_queued_runner_job_status(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "queued" | "pending" | "waiting" | "requested"
    )
}

fn count_jobs_by_repository(jobs: &[RunnerQueueJobSummary]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for job in jobs {
        *counts.entry(job.repository.clone()).or_insert(0) += 1;
    }
    counts
}

fn repository_matches_expected_scope(repository: &str, expected_scope: &str) -> bool {
    if expected_scope.ends_with('/') {
        repository.starts_with(expected_scope)
    } else {
        repository == expected_scope
    }
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

fn output_schema_audit(args: OutputSchemaAuditArgs) -> Result<()> {
    let report = output_schema_audit_report(&args.root)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if report.structured_output_ready {
        println!(
            "forge-loop output schema audit passed: {} required fields checked",
            report.required_fields.len()
        );
    } else {
        println!("forge-loop output schema audit failed:");
        for field in &report.missing_fields {
            println!("  - missing {field}");
        }
    }

    if !args.strict || report.structured_output_ready {
        Ok(())
    } else {
        Err(anyhow!(
            "forge-loop output schema missing required evidence: {}",
            report.missing_fields.join(", ")
        ))
    }
}

fn output_schema_audit_report(root: &Path) -> Result<OutputSchemaAuditReport> {
    let schema_path = ".github/codex/schemas/forge-loop-output.schema.json";
    let path = root.join(schema_path);
    let text =
        fs::read_to_string(&path).with_context(|| format!("read schema {}", path.display()))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parse schema {}", path.display()))?;
    let required_fields = required_output_schema_fields();
    let present_fields = required_fields
        .iter()
        .filter(|field| json_schema_requires_key(&parsed, field))
        .cloned()
        .collect::<Vec<_>>();
    let missing_fields = required_fields
        .iter()
        .filter(|field| !present_fields.contains(field))
        .cloned()
        .collect::<Vec<_>>();

    Ok(OutputSchemaAuditReport {
        schema_path,
        required_fields,
        present_fields,
        schema_valid_json: true,
        structured_output_ready: missing_fields.is_empty(),
        missing_fields,
    })
}

fn required_output_schema_fields() -> Vec<String> {
    [
        "summary",
        "auth_mode",
        "auth_evidence",
        "codex_home",
        "login_status_checked",
        "auth_json_present",
        "sources_mined",
        "component_inventory",
        "config",
        "hooks",
        "rules",
        "skills",
        "agents",
        "permissions",
        "github_action",
        "model_flags",
        "tool_surfaces",
        "structured_output_schemas",
        "auto_compaction_continuity_settings",
        "recommended_self_upgrade",
        "tests_required_before_merge",
        "verification",
        "auto_compact_continuity",
        "enabled",
        "compact_prompt",
        "preserved_state",
        "phases",
        "phases.minItems",
        "phases.maxItems",
        "phases.items.enum",
        "active_phase",
        "active_phase.enum",
        "current_phase_index",
        "current_phase_index.minimum",
        "source_coverage",
        "validation_state",
        "validation_terminal_state",
        "validation_sources",
        "phase_continuity",
        "phase_next_actions",
        "phase_validation_commands",
        "phase_validation_commands.Red.minItems",
        "phase_validation_commands.Implement.minItems",
        "phase_validation_commands.Gate.minItems",
        "phase_validation_commands.Evaluate.minItems",
        "phase_validation_commands.Research.minItems",
        "phase_validation_commands.Upgrade.minItems",
        "phase_validation_commands.Red.items.pattern",
        "phase_validation_commands.Implement.items.pattern",
        "phase_validation_commands.Gate.items.pattern",
        "phase_validation_commands.Evaluate.items.pattern",
        "phase_validation_commands.Research.items.pattern",
        "phase_validation_commands.Upgrade.items.pattern",
        "phase_validation_state",
        "phase_validation_state.Red.enum",
        "phase_validation_state.Implement.enum",
        "phase_validation_state.Gate.enum",
        "phase_validation_state.Evaluate.enum",
        "phase_validation_state.Research.enum",
        "phase_validation_state.Upgrade.enum",
        "next_action",
        "phase_source_validation_next_action",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn json_schema_requires_key(value: &serde_json::Value, key: &str) -> bool {
    if let Some(phase) = key
        .strip_prefix("phase_validation_commands.")
        .and_then(|rest| rest.strip_suffix(".minItems"))
    {
        return json_schema_array_min_items_for_path(
            value,
            &["phase_validation_commands", phase],
            1,
        );
    }
    if let Some(phase) = key
        .strip_prefix("phase_validation_commands.")
        .and_then(|rest| rest.strip_suffix(".items.pattern"))
    {
        return json_schema_array_items_pattern_for_path(
            value,
            &["phase_validation_commands", phase],
            "^rtk ",
        );
    }
    if let Some(phase) = key
        .strip_prefix("phase_validation_state.")
        .and_then(|rest| rest.strip_suffix(".enum"))
    {
        return json_schema_enum_contains_all_for_path(
            value,
            &["phase_validation_state", phase],
            &["pending", "in_progress", "passed", "failed"],
        );
    }
    if key == "active_phase.enum" {
        return json_schema_enum_contains_all_for_path(
            value,
            &["active_phase"],
            &[
                "Red",
                "Implement",
                "Gate",
                "Evaluate",
                "Research",
                "Upgrade",
            ],
        );
    }
    if key == "phases.minItems" {
        return json_schema_array_min_items_for_path(value, &["phases"], 6);
    }
    if key == "phases.maxItems" {
        return json_schema_array_max_items_for_path(value, &["phases"], 6);
    }
    if key == "phases.items.enum" {
        return json_schema_array_items_enum_contains_all_for_path(
            value,
            &["phases"],
            &[
                "Red",
                "Implement",
                "Gate",
                "Evaluate",
                "Research",
                "Upgrade",
            ],
        );
    }
    if key == "current_phase_index.minimum" {
        return json_schema_integer_minimum_for_path(value, &["current_phase_index"], 0);
    }

    match value {
        serde_json::Value::Object(map) => {
            map.get("required")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|required| {
                    required.iter().any(|item| {
                        item.as_str()
                            .is_some_and(|required_key| required_key == key)
                    })
                })
                || map
                    .values()
                    .any(|child| json_schema_requires_key(child, key))
        }
        serde_json::Value::Array(items) => items
            .iter()
            .any(|child| json_schema_requires_key(child, key)),
        _ => false,
    }
}

fn json_schema_enum_contains_all_for_path(
    value: &serde_json::Value,
    path: &[&str],
    required_values: &[&str],
) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            if path.is_empty() {
                let Some(values) = map.get("enum").and_then(serde_json::Value::as_array) else {
                    return false;
                };
                return required_values.iter().all(|required| {
                    values
                        .iter()
                        .any(|value| value.as_str().is_some_and(|actual| actual == *required))
                });
            }

            if let Some(properties) = map.get("properties").and_then(serde_json::Value::as_object) {
                if let Some(child) = properties.get(path[0]) {
                    if json_schema_enum_contains_all_for_path(child, &path[1..], required_values) {
                        return true;
                    }
                }
            }

            map.values()
                .any(|child| json_schema_enum_contains_all_for_path(child, path, required_values))
        }
        serde_json::Value::Array(items) => items
            .iter()
            .any(|child| json_schema_enum_contains_all_for_path(child, path, required_values)),
        _ => false,
    }
}

fn json_schema_array_items_enum_contains_all_for_path(
    value: &serde_json::Value,
    path: &[&str],
    required_values: &[&str],
) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            if path.is_empty() {
                let Some(items) = map.get("items") else {
                    return false;
                };
                return json_schema_enum_contains_all_for_path(items, &[], required_values);
            }

            if let Some(properties) = map.get("properties").and_then(serde_json::Value::as_object) {
                if let Some(child) = properties.get(path[0]) {
                    if json_schema_array_items_enum_contains_all_for_path(
                        child,
                        &path[1..],
                        required_values,
                    ) {
                        return true;
                    }
                }
            }

            map.values().any(|child| {
                json_schema_array_items_enum_contains_all_for_path(child, path, required_values)
            })
        }
        serde_json::Value::Array(items) => items.iter().any(|child| {
            json_schema_array_items_enum_contains_all_for_path(child, path, required_values)
        }),
        _ => false,
    }
}

fn json_schema_array_min_items_for_path(
    value: &serde_json::Value,
    path: &[&str],
    min_items: u64,
) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            if path.is_empty() {
                return map
                    .get("minItems")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|actual| actual >= min_items);
            }

            if let Some(properties) = map.get("properties").and_then(serde_json::Value::as_object) {
                if let Some(child) = properties.get(path[0]) {
                    if json_schema_array_min_items_for_path(child, &path[1..], min_items) {
                        return true;
                    }
                }
            }

            map.values()
                .any(|child| json_schema_array_min_items_for_path(child, path, min_items))
        }
        serde_json::Value::Array(items) => items
            .iter()
            .any(|child| json_schema_array_min_items_for_path(child, path, min_items)),
        _ => false,
    }
}

fn json_schema_array_max_items_for_path(
    value: &serde_json::Value,
    path: &[&str],
    max_items: u64,
) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            if path.is_empty() {
                return map
                    .get("maxItems")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|actual| actual <= max_items);
            }

            if let Some(properties) = map.get("properties").and_then(serde_json::Value::as_object) {
                if let Some(child) = properties.get(path[0]) {
                    if json_schema_array_max_items_for_path(child, &path[1..], max_items) {
                        return true;
                    }
                }
            }

            map.values()
                .any(|child| json_schema_array_max_items_for_path(child, path, max_items))
        }
        serde_json::Value::Array(items) => items
            .iter()
            .any(|child| json_schema_array_max_items_for_path(child, path, max_items)),
        _ => false,
    }
}

fn json_schema_array_items_pattern_for_path(
    value: &serde_json::Value,
    path: &[&str],
    required_pattern: &str,
) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            if path.is_empty() {
                return map
                    .get("items")
                    .and_then(|items| items.get("pattern"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|actual| actual == required_pattern);
            }

            if let Some(properties) = map.get("properties").and_then(serde_json::Value::as_object) {
                if let Some(child) = properties.get(path[0]) {
                    if json_schema_array_items_pattern_for_path(child, &path[1..], required_pattern)
                    {
                        return true;
                    }
                }
            }

            map.values().any(|child| {
                json_schema_array_items_pattern_for_path(child, path, required_pattern)
            })
        }
        serde_json::Value::Array(items) => items
            .iter()
            .any(|child| json_schema_array_items_pattern_for_path(child, path, required_pattern)),
        _ => false,
    }
}

fn json_schema_integer_minimum_for_path(
    value: &serde_json::Value,
    path: &[&str],
    minimum: i64,
) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            if path.is_empty() {
                return map
                    .get("minimum")
                    .and_then(serde_json::Value::as_i64)
                    .is_some_and(|actual| actual >= minimum);
            }

            if let Some(properties) = map.get("properties").and_then(serde_json::Value::as_object) {
                if let Some(child) = properties.get(path[0]) {
                    if json_schema_integer_minimum_for_path(child, &path[1..], minimum) {
                        return true;
                    }
                }
            }

            map.values()
                .any(|child| json_schema_integer_minimum_for_path(child, path, minimum))
        }
        serde_json::Value::Array(items) => items
            .iter()
            .any(|child| json_schema_integer_minimum_for_path(child, path, minimum)),
        _ => false,
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
            id: "openai-codex",
            url: "https://github.com/openai/codex",
            source_terms: &[
                "https://github.com/openai/codex",
                "Codex Rust CLI",
                "noninteractive execution",
            ],
            application_terms: &[
                ("crates/runner-cli/src/forge_loop.rs", "codex_invocation"),
                ("crates/runner-cli/src/forge_loop.rs", "--json"),
                ("crates/runner-cli/src/forge_loop.rs", "codex exec"),
            ],
            guard_terms: &[
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "codex_invocation_uses_noninteractive_json_workspace_write",
                ),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "target_mining_audit_covers_full_research_source_matrix",
                ),
            ],
        },
        TargetMiningTarget {
            id: "codex-config-advanced",
            url: "https://developers.openai.com/codex/config-advanced",
            source_terms: &[
                "https://developers.openai.com/codex/config-advanced",
                "Codex project config",
                "auto-compaction",
            ],
            application_terms: &[
                (".codex/config.toml", "auto_compaction = true"),
                (".codex/config.toml", "experimental_compact_prompt_file"),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "features.auto_compaction=true",
                ),
            ],
            guard_terms: &[
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "forge_loop_config_enables_auto_compaction",
                ),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "codex_invocation_forces_auto_compaction_continuity",
                ),
            ],
        },
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
                    "workflow_dispatch",
                ),
                (".github/workflows/codex-forge-loop.yml", "prompt_file:"),
                (
                    ".github/workflows/codex-forge-loop.yml",
                    "codex-forge-loop-output.md",
                ),
                (
                    ".github/workflows/codex-forge-loop.yml",
                    "local ChatGPT auth",
                ),
                (".github/workflows/codex-forge-loop.yml", "FXRUN_CODEX"),
                (
                    ".github/codex/schemas/forge-loop-output.schema.json",
                    "component_inventory",
                ),
                (
                    ".github/codex/schemas/forge-loop-output.schema.json",
                    "auto_compact_continuity",
                ),
                (
                    ".github/codex/schemas/forge-loop-output.schema.json",
                    "auth_mode",
                ),
            ],
            guard_terms: &[
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "codex_github_action_workflow_uses_documented_controls",
                ),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "output_schema_audit_requires_subscription_auth_inventory_and_continuity",
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
                (
                    ".codex/archive/lifecycle-hooks-20260703T024950Z/hooks.json.md",
                    "SubagentStart",
                ),
                (
                    ".codex/archive/lifecycle-hooks-20260703T024950Z/hooks.json.md",
                    "SubagentStop",
                ),
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
            id: "crates-io",
            url: "https://crates.io",
            source_terms: &[
                "https://crates.io",
                "Rust crates",
                "scheduling, tracing, structured output, evaluation, and reliability",
            ],
            application_terms: &[
                ("Cargo.toml", "workspace"),
                ("Cargo.lock", "serde_json"),
                ("Cargo.lock", "anyhow"),
                ("crates/runner-cli/src/forge_loop.rs", "serde_json"),
            ],
            guard_terms: &[
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "target_mining_audit_covers_full_research_source_matrix",
                ),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "doctor_json_exports_required_gate_contract",
                ),
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
                    "docs/forge-loop/kclaw0-runner-flow-target.md",
                    "runner-queue-audit --repo-jobs-json",
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
                    "runner_fleet_audit_default_scope_accepts_all_flexnetos_org_repos",
                ),
                (
                    "crates/runner-cli/src/forge_loop.rs",
                    "runner_queue_audit_classifies_local_waits_and_nonlocal_queues",
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
        permission_profile_readiness: permission_profile_readiness(root),
        checklist_shell_discipline: checklist_shell_discipline_readiness(root),
    }
}

fn checklist_shell_discipline_readiness(root: &Path) -> ChecklistShellDisciplineReadiness {
    let checklist_path = ".codex/checklists/forge-loop-cycle.toml";
    let checklist = fs::read_to_string(root.join(checklist_path)).unwrap_or_default();
    let mut checked_commands = Vec::new();
    let mut raw_command_keys = Vec::new();
    let mut raw_commands = Vec::new();
    for line in checklist.lines().map(str::trim) {
        let Some((key, value)) = line.split_once(" = ") else {
            continue;
        };
        if !required_checklist_command_keys().contains(&key) {
            continue;
        }
        checked_commands.push(key.to_string());
        if !value.starts_with("\"rtk ") {
            raw_command_keys.push(key.to_string());
            raw_commands.push((key.to_string(), unquote_toml_string(value)));
        }
    }

    let mut blockers = Vec::new();
    if checked_commands.len() != required_checklist_command_keys().len() {
        blockers
            .push("forge-loop checklist is missing one or more required command entries".into());
    }
    for (key, command) in &raw_commands {
        blockers.push(format!(
            "checklist command {key} is not rtk-prefixed; expected: rtk {command}"
        ));
    }

    ChecklistShellDisciplineReadiness {
        checklist_path,
        checked_commands,
        raw_command_keys,
        rtk_ready: blockers.is_empty(),
        blockers,
    }
}

fn required_checklist_command_keys() -> &'static [&'static str] {
    &[
        "component_audit",
        "target_mining_audit",
        "docs_drift",
        "forge_loop_tests",
        "workspace_tests",
        "clippy",
        "audit",
    ]
}

fn permission_profile_readiness(root: &Path) -> PermissionProfileReadiness {
    let config = fs::read_to_string(root.join(".codex/config.toml")).unwrap_or_default();
    let mirror = fs::read_to_string(root.join(".codex/permissions/forge-loop-workspace.toml"))
        .unwrap_or_default();
    let active_default_permissions = extract_quoted_toml_value(&config, "default_permissions");
    let active_sandbox_mode = extract_quoted_toml_value(&config, "sandbox_mode");
    let mirror_default_permissions = extract_quoted_toml_value(&mirror, "default_permissions");
    let profile_rules_present = [
        "[permissions.forge-loop-workspace.filesystem]",
        "\":minimal\" = \"read\"",
        "\":tmpdir\" = \"write\"",
        "\":slash_tmp\" = \"write\"",
        "[permissions.forge-loop-workspace.filesystem.\":workspace_roots\"]",
        "\".\" = \"write\"",
        "\".git\" = \"read\"",
        "\"**/*.env\" = \"deny\"",
        "\"**/*secret*\" = \"deny\"",
        "\"**/*token*\" = \"deny\"",
        "[permissions.forge-loop-workspace.network.domains]",
        "\"developers.openai.com\" = \"allow\"",
        "\"github.com\" = \"allow\"",
        "\"crates.io\" = \"allow\"",
    ]
    .iter()
    .all(|required| config.contains(required) || mirror.contains(required));

    let mut blockers = Vec::new();
    if active_default_permissions.as_deref() != Some("forge-loop-workspace") {
        blockers
            .push("active config does not select default_permissions=forge-loop-workspace".into());
    }
    if active_sandbox_mode.is_some() {
        blockers.push("active config still contains sandbox_mode".into());
    }
    if mirror_default_permissions.as_deref() != Some("forge-loop-workspace") {
        blockers.push(
            "permission profile mirror is missing default_permissions=forge-loop-workspace".into(),
        );
    }
    if !profile_rules_present {
        blockers.push("permission profile parity rules are incomplete".into());
    }

    PermissionProfileReadiness {
        active_default_permissions,
        active_sandbox_mode,
        mirror_default_permissions,
        profile_rules_present,
        migration_ready: blockers.is_empty(),
        blockers,
    }
}

fn extract_quoted_toml_value(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = ");
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(&prefix))
        .and_then(|value| value.trim().strip_prefix('"'))
        .and_then(|value| value.split('"').next())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn unquote_toml_string(value: &str) -> String {
    value
        .trim()
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value.trim())
        .to_string()
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
            id: "archived-hooks",
            surface: "hooks",
            path: ".codex/archive/lifecycle-hooks-20260703T024950Z/hooks.json.md",
            rationale: "Advanced Codex config supports repo-local hooks.json for lifecycle hooks, while this repo now preserves the removed lifecycle wiring in an auditable archive instead of activating duplicate root hooks.",
        },
        LoopComponent {
            id: "permission-request-hook",
            surface: "hooks",
            path: ".codex/hooks/forge_loop_permission_request.py",
            rationale: "Codex PermissionRequest hooks can witness approval posture while components-audit exposes permission-profile migration readiness.",
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
            rationale: "Codex permission profiles provide a least-privilege migration target and parity source for the active config readiness audit.",
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
            rationale: "Codex GitHub Action docs describe workflow prompt inputs, model/effort controls, output files, and structured evidence; this scheduled workflow applies those controls through local subscription-auth forge-loop execution.",
        },
        LoopComponent {
            id: "codex-output-schema",
            surface: "tools",
            path: ".github/codex/schemas/forge-loop-output.schema.json",
            rationale: "Codex GitHub Action docs allow structured output schemas; the repo keeps the evidence schema as the parity target for subscription-auth forge-loop output.",
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

fn codex_auth_readiness() -> CodexAuthReadiness {
    let codex_home = std::env::var("CODEX_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CODEX_HOME.into());
    let auth_json = Path::new(&codex_home).join("auth.json");
    let auth_json_display = auth_json.to_string_lossy().replace('\\', "/");
    CodexAuthReadiness {
        auth_mode: "local_chatgpt",
        codex_home,
        auth_json: auth_json_display.clone(),
        auth_json_present: auth_json.exists(),
        login_status_checked: false,
        login_status_command: "rtk codex login status",
        verification_commands: vec![
            "rtk codex login status".into(),
            format!("rtk proxy test -f {auth_json_display}"),
        ],
    }
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
            "--output-schema".into(),
            CODEX_OUTPUT_SCHEMA_PATH.into(),
            prompt,
        ],
    }
}

pub fn research_sources() -> Vec<ResearchSource> {
    vec![
        ResearchSource { id: "openai-codex", url: "https://github.com/openai/codex", purpose: "Codex Rust CLI behavior, noninteractive execution, JSONL, and upstream issues" },
        ResearchSource { id: "codex-config-advanced-docs", url: "https://developers.openai.com/codex/config-advanced", purpose: "Codex project config, hooks, rules, custom agents, model flags, sandbox, and auto-compaction settings" },
        ResearchSource { id: "codex-github-action-docs", url: "https://developers.openai.com/codex/github-action", purpose: "Codex Action prompt-file, codex-args, sandbox, safety-strategy, output, and structured schema controls" },
        ResearchSource { id: "codex-permissions-docs", url: "https://developers.openai.com/codex/permissions", purpose: "Permission-profile migration, filesystem/network least privilege, and sandbox/profile non-composition rules" },
        ResearchSource { id: "codex-subagents-docs", url: "https://developers.openai.com/codex/subagents", purpose: "Project custom agents, explicit fan-out, inherited sandbox behavior, and max thread/depth controls" },
        ResearchSource { id: "awesome-codex-cli", url: "https://github.com/RoggeOhta/awesome-codex-cli", purpose: "Codex ecosystem tools, skills, plugins, MCP servers, and orchestration patterns" },
        ResearchSource { id: "oh-my-codex", url: "https://github.com/Yeachan-Heo/oh-my-codex", purpose: "multi-agent teams, hooks, HUDs, and Codex orchestration UX" },
        ResearchSource { id: "crates-io", url: "https://crates.io", purpose: "Rust crates that improve loop reliability, accuracy, speed, tracing, and scheduling" },
        ResearchSource { id: "kclaw0", url: "https://github.com/drdave-flexnetos/kclaw0", purpose: "local dark-factory/self-upgrade prior art and governance patterns" },
        ResearchSource { id: "kclaw0-upgrade-ledger", url: "docs/kclaw0-upgrade-ledger.md", purpose: "local applied-governance ledger for strict-upgrade parity, validation, and prior-art continuity" },
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

fn validate_eval_manifest_pair(input: &EvalInput, manifest: &CycleManifest) -> Result<()> {
    match input.cycle_goal.as_deref() {
        Some(goal) if goal == manifest.goal => {}
        Some(goal) => {
            return Err(anyhow!(
                "cycle_goal {:?} does not match manifest goal {:?}",
                goal,
                manifest.goal
            ));
        }
        None => {
            return Err(anyhow!(
                "cycle_goal is required when --manifest is provided"
            ))
        }
    }

    match input.prompt_sha256.as_deref() {
        Some(hash) if hash == manifest.prompt_sha256 => Ok(()),
        Some(hash) => Err(anyhow!(
            "prompt_sha256 {:?} does not match manifest prompt_sha256 {:?}",
            hash,
            manifest.prompt_sha256
        )),
        None => Err(anyhow!(
            "prompt_sha256 is required when --manifest is provided"
        )),
    }
}

impl EvalInput {
    pub fn fixture() -> Self {
        let cycle_goal = "scheduled subscription-auth Codex self-improvement".to_string();
        Self {
            prompt_sha256: Some(runner_core::constitution::hash(
                cycle_prompt(&cycle_goal, true).as_bytes(),
            )),
            cycle_goal: Some(cycle_goal),
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
        "Run a Codex TDD forge-loop cycle for this Rust repo. Goal: {goal}. Do not start another cycle. Verify local ChatGPT subscription auth before implementation with `rtk codex login status` and `rtk proxy test -f /home/flexnetos/.codex/auth.json`. Keep auto-compaction enabled and preserve phase/source/validation/next-action continuity in compact summaries. Required phases: write/verify a red test first, implement the smallest passing change, run fmt/clippy/tests/audit, evaluate the run, and research one reliability/accuracy/speed improvement. If a self-upgrade is warranted, leave the intended repository changes in the working tree; do not run git commit, git push, or gh pr from inside Codex. The outer forge-loop engine will commit, push, open a PR with PR title '{pr_title}', and {}. Strict upgrade only: no downgrades or removals without installed replacement and parity proof. Shell discipline: prefix every shell command with `rtk`; for Unix `find` with compound predicates or actions, use `rtk proxy find ...` instead of `rtk find ...` because `rtk find` rejects compound predicates.",
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
    let output_contract = research_output_contract()
        .into_iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Research Codex forge-loop improvements focused on {focus}. Scan these references and return actionable, source-attributed upgrades for reliability, accuracy, and speed:\n{list}\n\nOutput format:\n{output_contract}"
    )
}

fn publish_self_upgrade_if_needed(
    pr_title: &str,
    auto_merge: bool,
    log: &Path,
) -> Result<Option<String>> {
    let status = run_command("git", &["status", "--porcelain", "--untracked-files=all"])?;
    let paths = publishable_paths_from_status(&status);
    if paths.is_empty() {
        return Ok(None);
    }

    let branch = format!("codex/forge-loop-self-upgrade-{}", timestamp_label()?);
    run_command("git", &["switch", "-c", &branch])?;

    let mut add_args = vec!["add".to_string(), "--".to_string()];
    add_args.extend(paths.iter().cloned());
    run_command_owned("git", add_args)?;

    run_command(
        "git",
        &[
            "-c",
            "user.name=codex-forge-loop",
            "-c",
            "user.email=codex-forge-loop@users.noreply.github.com",
            "commit",
            "-m",
            pr_title,
        ],
    )?;
    run_command("git", &["push", "-u", "origin", &branch])?;

    let body = format!(
        "Automated forge-loop self-upgrade.\n\nPR title: `{pr_title}`\n\nThe inner Codex session ran inside the workspace-write sandbox and left publishable repository changes in the working tree. The outer forge-loop engine committed and opened this PR so `.git` remains outside the nested Codex write surface."
    );
    let pr_create_output = run_command(
        "gh",
        &[
            "pr", "create", "--base", "main", "--head", &branch, "--title", pr_title, "--body",
            &body,
        ],
    )?;
    let pr_url = parse_pr_create_reference(&pr_create_output)?;
    append_event(
        log,
        CycleEvent {
            event: "publish.pr_created",
            phase: CyclePhase::Upgrade,
            detail: &pr_url,
        },
    )?;

    dispatch_required_checks(&branch, pr_title, log)?;

    if auto_merge {
        run_command(
            "gh",
            &[
                "pr",
                "merge",
                &pr_url,
                "--auto",
                "--squash",
                "--delete-branch",
            ],
        )?;
        append_event(
            log,
            CycleEvent {
                event: "publish.auto_merge_requested",
                phase: CyclePhase::Upgrade,
                detail: &pr_url,
            },
        )?;
    }

    Ok(Some(pr_url))
}

fn parse_pr_create_reference(output: &str) -> Result<String> {
    let trimmed = output.trim();
    for token in trimmed.split_whitespace().rev() {
        let token = token.trim_matches(|c: char| matches!(c, ',' | '.' | ';' | '(' | ')'));
        if (token.starts_with("https://") || token.starts_with("http://"))
            && token.contains("/pull/")
        {
            return Ok(token.to_string());
        }
    }
    for token in trimmed.split_whitespace() {
        let token = token.trim_matches(|c: char| matches!(c, ',' | '.' | ';' | '(' | ')'));
        if let Some(number) = token.strip_prefix('#') {
            if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) {
                return Ok(number.to_string());
            }
        }
        if !token.is_empty() && token.chars().all(|c| c.is_ascii_digit()) {
            return Ok(token.to_string());
        }
    }
    Err(anyhow!(
        "could not parse PR reference from gh pr create output: {trimmed:?}"
    ))
}

fn dispatch_required_checks(branch: &str, pr_title: &str, log: &Path) -> Result<()> {
    run_command(
        "gh",
        &[
            "workflow",
            "run",
            REQUIRED_CHECK_WORKFLOWS[0],
            "--ref",
            branch,
        ],
    )?;
    let ci_detail = format!("{}@{}", REQUIRED_CHECK_WORKFLOWS[0], branch);
    append_event(
        log,
        CycleEvent {
            event: "publish.required_check_dispatched",
            phase: CyclePhase::Gate,
            detail: &ci_detail,
        },
    )?;

    let title_input = format!("{SEMANTIC_PR_TITLE_INPUT}={pr_title}");
    run_command(
        "gh",
        &[
            "workflow",
            "run",
            REQUIRED_CHECK_WORKFLOWS[1],
            "--ref",
            branch,
            "-f",
            &title_input,
        ],
    )?;
    let semantic_detail = format!(
        "{}@{} {}",
        REQUIRED_CHECK_WORKFLOWS[1], SEMANTIC_PR_TITLE_INPUT, branch
    );
    append_event(
        log,
        CycleEvent {
            event: "publish.required_check_dispatched",
            phase: CyclePhase::Gate,
            detail: &semantic_detail,
        },
    )?;

    Ok(())
}

fn publishable_paths_from_status(status: &str) -> Vec<String> {
    status
        .lines()
        .filter_map(|line| line.get(3..))
        .map(str::trim)
        .filter_map(|path| path.split(" -> ").last())
        .map(normalize_git_porcelain_path)
        .filter(|path| *path != CODEX_FORGE_LOOP_OUTPUT)
        .filter(|path| !path.starts_with("_work/"))
        .filter(|path| !path.is_empty())
        .collect()
}

fn normalize_git_porcelain_path(path: &str) -> String {
    if !(path.starts_with('"') && path.ends_with('"') && path.len() >= 2) {
        return path.to_string();
    }

    let mut normalized = String::new();
    let mut chars = path[1..path.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            normalized.push(ch);
            continue;
        }
        match chars.next() {
            Some('"') => normalized.push('"'),
            Some('\\') => normalized.push('\\'),
            Some('n') => normalized.push('\n'),
            Some('t') => normalized.push('\t'),
            Some(other) => {
                normalized.push('\\');
                normalized.push(other);
            }
            None => normalized.push('\\'),
        }
    }
    normalized
}

fn run_command(program: &str, args: &[&str]) -> Result<String> {
    run_command_owned(program, args.iter().map(|arg| (*arg).to_string()).collect())
}

fn run_command_owned(program: &str, args: Vec<String>) -> Result<String> {
    let (wrapped_program, wrapped_args) = command_invocation(program, args);
    let output = Command::new(&wrapped_program)
        .args(&wrapped_args)
        .output()
        .with_context(|| format!("spawn {wrapped_program} {}", wrapped_args.join(" ")))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{wrapped_program} {} failed with status {}: {}",
            wrapped_args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn command_invocation(program: &str, args: Vec<String>) -> (String, Vec<String>) {
    let mut wrapped_args = Vec::with_capacity(args.len() + 2);
    if program == "find" {
        wrapped_args.push("proxy".to_string());
    }
    wrapped_args.push(program.to_string());
    wrapped_args.extend(args);
    ("rtk".into(), wrapped_args)
}

fn compact_continuity_artifact() -> CompactContinuityArtifact {
    CompactContinuityArtifact {
        enabled: true,
        compact_prompt: COMPACT_PROMPT_PATH.into(),
        compact_summary_events: vec!["PreCompact".into(), "PostCompact".into()],
        phases: required_phases(),
        active_phase: CyclePhase::Red,
        current_phase_index: 0,
        source_coverage: research_sources()
            .into_iter()
            .map(|source| format!("{}: {} ({})", source.id, source.url, source.purpose))
            .collect(),
        research_output_contract: research_output_contract(),
        validation_state: REQUIRED_GATE_COMMANDS
            .iter()
            .map(|command| format!("pending: {command}"))
            .collect(),
        validation_terminal_state: REQUIRED_GATE_COMMANDS
            .iter()
            .map(|command| format!("passed: {command}"))
            .collect(),
        validation_sources: compact_validation_source_entries(),
        phase_continuity: phase_continuity_entries(),
        phase_next_actions: phase_next_actions(),
        phase_validation_commands: phase_validation_commands(),
        phase_validation_state: phase_validation_state(),
        next_action: "continue with the next required forge-loop phase".into(),
        phase_source_validation_next_action:
            "phase=Red source_coverage=complete validation_state=pending next_action=continue"
                .into(),
    }
}

fn phase_validation_state() -> BTreeMap<String, String> {
    required_phases()
        .into_iter()
        .map(|phase| (cycle_phase_label(phase).to_string(), "pending".to_string()))
        .collect()
}

fn phase_validation_commands() -> BTreeMap<String, Vec<String>> {
    let mut commands: BTreeMap<String, Vec<String>> = BTreeMap::new();
    commands.insert(
        cycle_phase_label(CyclePhase::Red).to_string(),
        vec![
            "rtk cargo test -p runner-cli --all-features <new_red_test_name> -- --nocapture"
                .to_string(),
        ],
    );
    commands.insert(
        cycle_phase_label(CyclePhase::Implement).to_string(),
        vec![
            "rtk cargo test -p runner-cli --all-features <new_red_test_name> -- --nocapture"
                .to_string(),
        ],
    );
    for command in REQUIRED_GATE_COMMANDS {
        commands
            .entry(cycle_phase_label(validation_phase_for_command(command)).to_string())
            .or_default()
            .push((*command).to_string());
    }
    commands
}

fn phase_next_actions() -> BTreeMap<String, String> {
    required_phases()
        .into_iter()
        .map(|phase| {
            (
                cycle_phase_label(phase).to_string(),
                continuity_next_action_for_phase(phase).to_string(),
            )
        })
        .collect()
}

fn phase_continuity_entries() -> Vec<String> {
    required_phases()
        .into_iter()
        .map(|phase| {
            format!(
                "phase={} source={} validation_state=pending next_action={}",
                cycle_phase_label(phase),
                continuity_source_for_phase(phase),
                continuity_next_action_for_phase(phase)
            )
        })
        .collect()
}

fn continuity_source_for_phase(phase: CyclePhase) -> &'static str {
    match phase {
        CyclePhase::Red => "red_test_evidence",
        CyclePhase::Implement => "working_tree_diff",
        CyclePhase::Gate => "required_gate_commands",
        CyclePhase::Evaluate => "evaluation_artifacts",
        CyclePhase::Research => "research_sources",
        CyclePhase::Upgrade => "strict_upgrade_plan",
    }
}

fn continuity_next_action_for_phase(phase: CyclePhase) -> &'static str {
    match phase {
        CyclePhase::Red => "implement_smallest_passing_change",
        CyclePhase::Implement => "run_required_gates",
        CyclePhase::Gate => "evaluate_run",
        CyclePhase::Evaluate => "research_reliability_accuracy_speed_improvement",
        CyclePhase::Research => "decide_smallest_safe_self_upgrade",
        CyclePhase::Upgrade => "leave_publishable_changes_for_outer_engine",
    }
}

fn validation_source_entries() -> Vec<String> {
    REQUIRED_GATE_COMMANDS
        .iter()
        .map(|command| {
            format!(
                "phase={} source=required_gate_commands validation_state=pending command={command}",
                cycle_phase_label(validation_phase_for_command(command))
            )
        })
        .collect()
}

fn compact_validation_source_entries() -> Vec<String> {
    let mut entries = validation_source_entries();
    entries.extend(
        codex_auth_readiness()
            .verification_commands
            .into_iter()
            .map(|command| {
                format!(
                    "phase=Red source=subscription_auth validation_state=pending command={command}"
                )
            }),
    );
    entries
}

fn validation_phase_for_command(command: &str) -> CyclePhase {
    if command.contains("forge-loop eval ") {
        CyclePhase::Evaluate
    } else if command.contains("forge-loop research ") {
        CyclePhase::Research
    } else if command.contains("forge-loop self-upgrade ") {
        CyclePhase::Upgrade
    } else {
        CyclePhase::Gate
    }
}

fn cycle_phase_label(phase: CyclePhase) -> &'static str {
    match phase {
        CyclePhase::Red => "Red",
        CyclePhase::Implement => "Implement",
        CyclePhase::Gate => "Gate",
        CyclePhase::Evaluate => "Evaluate",
        CyclePhase::Research => "Research",
        CyclePhase::Upgrade => "Upgrade",
    }
}

fn research_output_contract() -> Vec<String> {
    [
        "one-line summary",
        "source-attributed findings",
        "loop component/config inventory: config, hooks, rules, skills, custom agents/subagents, permission profiles, model flags, GitHub Action/tool surfaces, structured output schemas, auto-compaction/continuity settings",
        "one recommended smallest safe self-upgrade",
        "tests required before merge",
    ]
    .into_iter()
    .map(String::from)
    .collect()
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
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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
    fn codex_invocation_enforces_structured_output_schema() {
        let inv = codex_invocation("do work".into());

        assert!(inv.args.windows(2).any(|w| w
            == [
                "--output-schema",
                ".github/codex/schemas/forge-loop-output.schema.json"
            ]));
    }

    #[test]
    fn doctor_exports_subscription_auth_readiness_contract() {
        let auth = codex_auth_readiness();

        assert_eq!(auth.auth_mode, "local_chatgpt");
        assert!(!auth.codex_home.is_empty());
        assert!(auth.auth_json.ends_with("auth.json"));
        assert_eq!(auth.login_status_command, "rtk codex login status");
    }

    #[test]
    fn codex_auth_readiness_exports_subscription_verification_commands() {
        let auth = codex_auth_readiness();

        assert!(
            auth.verification_commands
                .iter()
                .any(|command| command == "rtk codex login status"),
            "auth readiness should include the subscription login status proof command"
        );
        assert!(
            auth.verification_commands
                .iter()
                .any(|command| command.contains("auth.json")),
            "auth readiness should include a non-secret auth.json presence proof command"
        );
        assert!(
            auth.verification_commands
                .iter()
                .all(|command| command.starts_with("rtk ")),
            "auth readiness verification commands must obey forge-loop shell discipline"
        );
    }

    #[test]
    fn compact_continuity_preserves_subscription_auth_verification_commands() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let expected_commands = codex_auth_readiness().verification_commands;
        let continuity = compact_continuity_artifact();

        for command in expected_commands {
            assert!(
                continuity
                    .validation_sources
                    .iter()
                    .any(|source| source.contains(&command)),
                "compact continuity validation sources should preserve auth proof command: {command}"
            );
        }
    }

    #[test]
    fn subscription_auth_readiness_defaults_to_scheduled_codex_home() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let previous = std::env::var("CODEX_HOME").ok();
        std::env::remove_var("CODEX_HOME");

        let auth = codex_auth_readiness();

        if let Some(previous) = previous {
            std::env::set_var("CODEX_HOME", previous);
        }

        assert_eq!(auth.codex_home, "/home/flexnetos/.codex");
        assert_eq!(auth.auth_json, "/home/flexnetos/.codex/auth.json");
    }

    #[test]
    fn research_sources_include_required_refs() {
        let ids = research_sources()
            .into_iter()
            .map(|s| s.id)
            .collect::<Vec<_>>();
        assert!(ids.contains(&"openai-codex"));
        assert!(ids.contains(&"codex-config-advanced-docs"));
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
    fn eval_manifest_rejects_metrics_from_different_cycle_prompt() {
        let manifest = cycle_manifest(&RunArgs {
            goal: "scheduled subscription-auth Codex self-improvement".into(),
            out: PathBuf::from("_work/forge-loop"),
            dry_run: false,
            auto_merge: true,
            once: true,
        });
        let other_goal = "cycle 19 unrelated target".to_string();
        let metrics = EvalInput {
            cycle_goal: Some(other_goal.clone()),
            prompt_sha256: Some(runner_core::constitution::hash(
                cycle_prompt(&other_goal, manifest.auto_merge).as_bytes(),
            )),
            ..EvalInput::fixture()
        };

        let error = validate_eval_manifest_pair(&metrics, &manifest)
            .expect_err("metrics from a different prompt contract must fail");
        assert!(
            error.to_string().contains("cycle_goal"),
            "error should name the mismatched run contract: {error}"
        );
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
            ..EvalInput::fixture()
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
                "schema_version": 1,
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
    fn eval_manifest_rejects_missing_schema_version() {
        let path = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-missing-schema-{}.json",
            std::process::id()
        ));
        fs::write(
            &path,
            r#"{
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

        let error = parse_cycle_manifest(&path).expect_err("missing schema must fail");
        assert!(
            error.root_cause().to_string().contains("schema_version"),
            "error should name the missing schema version: {error}"
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
        assert!(prompt.contains("leave the intended repository changes in the working tree"));
        assert!(prompt.contains("do not run git commit, git push, or gh pr from inside Codex"));
        assert!(prompt.contains("The outer forge-loop engine will commit, push, open a PR"));
        assert!(prompt.contains("rtk proxy find"));
    }

    #[test]
    fn cycle_prompt_requires_subscription_auth_verification_before_implementation() {
        let prompt = cycle_prompt("scheduled subscription-auth Codex self-improvement", true);

        assert!(prompt.contains("Verify local ChatGPT subscription auth before implementation"));
        assert!(prompt.contains("rtk codex login status"));
        assert!(prompt.contains("rtk proxy test -f /home/flexnetos/.codex/auth.json"));
    }

    #[test]
    fn self_upgrade_prompt_leaves_publish_to_outer_engine() {
        let prompt = self_upgrade_prompt(90);
        let prompt_lower = prompt.to_ascii_lowercase();

        assert!(prompt_lower.contains("leave the intended repository changes in the working tree"));
        assert!(prompt.contains("do not run git commit, git push, or gh pr from inside Codex"));
        assert!(prompt.contains("The outer forge-loop engine will commit, push, open a PR"));
        assert!(prompt.contains("rtk proxy find"));
        assert!(
            !prompt.contains("Commit, push, open a PR"),
            "self-upgrade prompt must not ask nested Codex to publish"
        );
    }

    #[test]
    fn publishable_paths_ignore_runtime_artifacts() {
        let paths = publishable_paths_from_status(
            r#" M crates/runner-cli/src/forge_loop.rs
 M docs/kclaw0-upgrade-ledger.md
?? codex-forge-loop-output.md
?? _work/forge-loop/cycle/events.jsonl
?? docs/forge-loop/new-artifact.md
R  docs/old.md -> docs/new.md
"#,
        );

        assert_eq!(
            paths,
            vec![
                "crates/runner-cli/src/forge_loop.rs".to_string(),
                "docs/kclaw0-upgrade-ledger.md".to_string(),
                "docs/forge-loop/new-artifact.md".to_string(),
                "docs/new.md".to_string(),
            ]
        );
    }

    #[test]
    fn publishable_paths_unquote_git_porcelain_paths_with_spaces() {
        let paths = publishable_paths_from_status(
            r#"?? "docs/forge-loop/research note.md"
R  "docs/old note.md" -> "docs/new note.md"
"#,
        );

        assert_eq!(
            paths,
            vec![
                "docs/forge-loop/research note.md".to_string(),
                "docs/new note.md".to_string(),
            ]
        );
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
    fn dry_run_writes_compact_continuity_artifact() {
        let out = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-compact-continuity-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&out).ok();

        run(RunArgs {
            goal: "scheduled subscription-auth Codex self-improvement".into(),
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
        let continuity = fs::read_to_string(cycle_dir.join("compact-continuity.json"))
            .expect("compact continuity artifact");
        let parsed: CompactContinuityArtifact =
            serde_json::from_str(&continuity).expect("compact continuity json");

        assert!(parsed.enabled);
        assert_eq!(parsed.compact_prompt, COMPACT_PROMPT_PATH);
        assert_eq!(parsed.active_phase, CyclePhase::Red);
        for source in research_sources() {
            assert!(
                parsed
                    .source_coverage
                    .iter()
                    .any(|entry| entry.contains(source.id) && entry.contains(source.url)),
                "compact continuity artifact missing {}",
                source.url
            );
        }
        assert!(parsed.validation_state.contains(
            &"pending: rtk cargo clippy --workspace --all-targets --all-features -- -D warnings"
                .to_string()
        ));
        assert!(parsed
            .next_action
            .contains("next required forge-loop phase"));

        fs::remove_dir_all(out).ok();
    }

    #[test]
    fn dry_run_writes_required_gate_artifact() {
        let out = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-required-gates-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&out).ok();

        run(RunArgs {
            goal: "scheduled subscription-auth Codex self-improvement".into(),
            out: out.clone(),
            dry_run: true,
            auto_merge: true,
            once: true,
        })
        .expect("dry run");

        let gates = fs::read_to_string(out.join("cycle/required-gates.json"))
            .expect("required gate artifact");
        let parsed: Vec<String> = serde_json::from_str(&gates).expect("required gates json");

        assert_eq!(parsed, REQUIRED_GATE_COMMANDS);
        assert!(parsed.iter().all(|command| command.starts_with("rtk ")));
        assert!(parsed
            .iter()
            .any(|command| command == "rtk cargo audit --deny warnings"));
        assert!(parsed.iter().any(|command| command
            == "rtk cargo clippy --workspace --all-targets --all-features -- -D warnings"));

        fs::remove_dir_all(out).ok();
    }

    #[test]
    fn dry_run_writes_subscription_auth_readiness_artifact() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let previous = std::env::var("CODEX_HOME").ok();
        std::env::remove_var("CODEX_HOME");
        let out = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-auth-readiness-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&out).ok();

        run(RunArgs {
            goal: "scheduled subscription-auth Codex self-improvement".into(),
            out: out.clone(),
            dry_run: true,
            auto_merge: true,
            once: true,
        })
        .expect("dry run");

        let auth = fs::read_to_string(out.join("cycle/codex-auth-readiness.json"))
            .expect("subscription auth readiness artifact");
        let parsed: serde_json::Value =
            serde_json::from_str(&auth).expect("subscription auth readiness json");

        assert_eq!(parsed["auth_mode"], "local_chatgpt");
        assert_eq!(parsed["codex_home"], DEFAULT_CODEX_HOME);
        assert_eq!(
            parsed["auth_json"],
            format!("{DEFAULT_CODEX_HOME}/auth.json")
        );
        assert_eq!(parsed["login_status_command"], "rtk codex login status");
        let verification_commands = parsed["verification_commands"]
            .as_array()
            .expect("verification commands");
        assert!(verification_commands
            .iter()
            .any(|command| command == "rtk codex login status"));
        assert!(verification_commands
            .iter()
            .filter_map(|command| command.as_str())
            .all(|command| command.starts_with("rtk ")));

        if let Some(previous) = previous {
            std::env::set_var("CODEX_HOME", previous);
        }
        fs::remove_dir_all(out).ok();
    }

    #[test]
    fn dry_run_auth_readiness_distinguishes_file_presence_from_login_status_check() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let previous = std::env::var("CODEX_HOME").ok();
        let codex_home = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-empty-codex-home-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let out = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-auth-proof-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::remove_dir_all(&codex_home).ok();
        fs::remove_dir_all(&out).ok();
        fs::create_dir_all(&codex_home).expect("codex home");
        std::env::set_var("CODEX_HOME", &codex_home);

        run(RunArgs {
            goal: "scheduled subscription-auth Codex self-improvement".into(),
            out: out.clone(),
            dry_run: true,
            auto_merge: true,
            once: true,
        })
        .expect("dry run");

        let auth = fs::read_to_string(out.join("cycle/codex-auth-readiness.json"))
            .expect("subscription auth readiness artifact");
        let parsed: serde_json::Value =
            serde_json::from_str(&auth).expect("subscription auth readiness json");

        assert_eq!(
            parsed["codex_home"].as_str(),
            Some(codex_home.to_string_lossy().as_ref())
        );
        assert_eq!(parsed["auth_json_present"], false);
        assert_eq!(parsed["login_status_checked"], false);
        assert!(
            parsed["verification_commands"]
                .as_array()
                .expect("verification commands")
                .iter()
                .any(|command| command == "rtk codex login status"),
            "artifact must preserve the explicit subscription login command for the caller to run"
        );

        if let Some(previous) = previous {
            std::env::set_var("CODEX_HOME", previous);
        } else {
            std::env::remove_var("CODEX_HOME");
        }
        fs::remove_dir_all(out).ok();
        fs::remove_dir_all(codex_home).ok();
    }

    #[test]
    fn dry_run_event_stream_preserves_compact_continuity_checkpoint() {
        let out = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-continuity-events-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&out).ok();

        run(RunArgs {
            goal: "scheduled subscription-auth Codex self-improvement".into(),
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
        let events = fs::read_to_string(cycle_dir.join("events.jsonl")).expect("events");

        assert!(
            events.contains("\"event\":\"continuity.compact_checkpoint\""),
            "dry-run event stream must include a compact continuity checkpoint"
        );
        assert!(
            events.contains(
                "phase=Red source_coverage=complete validation_state=pending next_action=continue"
            ),
            "compact checkpoint must preserve phase/source/validation/next-action continuity"
        );

        fs::remove_dir_all(out).ok();
    }

    #[test]
    fn compact_continuity_artifact_exports_structured_required_phase_contract() {
        let artifact = compact_continuity_artifact();

        assert_eq!(artifact.phases, required_phases());
        assert_eq!(artifact.current_phase_index, 0);
        assert_eq!(artifact.active_phase, CyclePhase::Red);
        assert!(artifact.phase_source_validation_next_action.contains(
            "phase=Red source_coverage=complete validation_state=pending next_action=continue"
        ));
    }

    #[test]
    fn compact_continuity_artifact_exports_per_phase_continuity_timeline() {
        let artifact = compact_continuity_artifact();

        assert_eq!(artifact.phase_continuity.len(), required_phases().len());
        for phase in required_phases() {
            let label = cycle_phase_label(phase);
            assert!(
                artifact.phase_continuity.iter().any(|entry| {
                    entry.contains(&format!("phase={label}"))
                        && entry.contains("source=")
                        && entry.contains("validation_state=")
                        && entry.contains("next_action=")
                }),
                "compact continuity artifact missing full continuity entry for {label}"
            );
        }
    }

    #[test]
    fn compact_continuity_artifact_exports_structured_phase_next_actions() {
        let artifact = compact_continuity_artifact();
        let value = serde_json::to_value(&artifact).expect("compact continuity json");
        let phase_next_actions = value
            .get("phase_next_actions")
            .and_then(serde_json::Value::as_object)
            .expect("compact continuity artifact must expose structured phase next actions");

        assert_eq!(phase_next_actions.len(), required_phases().len());
        for phase in required_phases() {
            let label = cycle_phase_label(phase);
            assert_eq!(
                phase_next_actions
                    .get(label)
                    .and_then(serde_json::Value::as_str),
                Some(continuity_next_action_for_phase(phase)),
                "compact continuity artifact missing structured next action for {label}"
            );
        }
    }

    #[test]
    fn compact_continuity_artifact_exports_phase_validation_commands() {
        let artifact = compact_continuity_artifact();
        let value = serde_json::to_value(&artifact).expect("compact continuity json");
        let phase_validation_commands = value
            .get("phase_validation_commands")
            .and_then(serde_json::Value::as_object)
            .expect("compact continuity artifact must expose structured phase validation commands");

        for phase in ["Gate", "Evaluate"] {
            let commands = phase_validation_commands
                .get(phase)
                .and_then(serde_json::Value::as_array)
                .expect("phase validation command list");
            assert!(
                commands.iter().all(|command| command
                    .as_str()
                    .is_some_and(|command| command.starts_with("rtk "))),
                "{phase} validation commands must preserve rtk shell discipline"
            );
        }
        assert!(
            phase_validation_commands["Gate"]
                .as_array()
                .expect("gate validation commands")
                .iter()
                .any(|command| command
                    == "rtk cargo run -q -p runner-cli -- forge-loop target-mining-audit --strict"),
            "Gate phase must retain strict target-mining validation"
        );
        assert!(
            phase_validation_commands["Evaluate"]
                .as_array()
                .expect("evaluate validation commands")
                .iter()
                .any(|command| command
                    == "rtk cargo run -q -p runner-cli -- forge-loop eval --fixture"),
            "Evaluate phase must retain deterministic eval validation"
        );
    }

    #[test]
    fn compact_continuity_artifact_exports_all_phase_validation_commands() {
        let artifact = compact_continuity_artifact();

        for phase in required_phases() {
            let label = cycle_phase_label(phase);
            let commands = artifact
                .phase_validation_commands
                .get(label)
                .unwrap_or_else(|| panic!("{label} phase missing validation commands"));
            assert!(
                !commands.is_empty(),
                "{label} phase must preserve at least one validation command"
            );
        }
    }

    #[test]
    fn compact_continuity_artifact_exports_structured_phase_validation_state() {
        let artifact = compact_continuity_artifact();
        let value = serde_json::to_value(&artifact).expect("compact continuity json");
        let phase_validation_state = value
            .get("phase_validation_state")
            .and_then(serde_json::Value::as_object)
            .expect("compact continuity artifact must expose structured phase validation state");

        assert_eq!(phase_validation_state.len(), required_phases().len());
        for phase in required_phases() {
            let label = cycle_phase_label(phase);
            assert_eq!(
                phase_validation_state
                    .get(label)
                    .and_then(serde_json::Value::as_str),
                Some("pending"),
                "compact continuity artifact missing pending status for {label}"
            );
        }
    }

    #[test]
    fn compact_continuity_artifact_labels_validation_entries_as_pending() {
        let artifact = compact_continuity_artifact();

        for gate in REQUIRED_GATE_COMMANDS {
            let expected = format!("pending: {gate}");
            assert!(
                artifact.validation_state.contains(&expected),
                "compact continuity artifact missing pending validation entry {expected}"
            );
        }
    }

    #[test]
    fn compact_continuity_artifact_exports_terminal_validation_contract() {
        let artifact = compact_continuity_artifact();

        assert_eq!(
            artifact.validation_terminal_state.len(),
            REQUIRED_GATE_COMMANDS.len()
        );
        for gate in REQUIRED_GATE_COMMANDS {
            let expected = format!("passed: {gate}");
            assert!(
                artifact.validation_terminal_state.contains(&expected),
                "compact continuity artifact missing terminal validation entry {expected}"
            );
        }
    }

    #[test]
    fn compact_continuity_artifact_attributes_evaluation_validation_source() {
        let artifact = compact_continuity_artifact();

        assert!(
            artifact.validation_sources.iter().any(|entry| entry
                == "phase=Evaluate source=required_gate_commands validation_state=pending command=rtk cargo run -q -p runner-cli -- forge-loop eval --fixture"),
            "compact continuity artifact must preserve eval phase/source validation continuity"
        );
    }

    #[test]
    fn compact_continuity_artifact_attributes_metrics_evaluation_validation_source() {
        let artifact = compact_continuity_artifact();
        let metrics_eval_command = "rtk cargo run -q -p runner-cli -- forge-loop eval --metrics /tmp/fxrun-forge-loop-gate-dry-run/cycle/evaluation-input.json --manifest /tmp/fxrun-forge-loop-gate-dry-run/cycle/cycle-manifest.json";

        assert!(
            artifact
                .phase_validation_commands
                .get("Evaluate")
                .is_some_and(|commands| commands.contains(&metrics_eval_command.to_string())),
            "compact continuity artifact must keep metrics/manifest eval validation under Evaluate"
        );
        assert!(
            artifact.validation_sources.iter().any(|entry| entry
                == &format!("phase=Evaluate source=required_gate_commands validation_state=pending command={metrics_eval_command}")),
            "compact continuity artifact must preserve metrics/manifest eval phase/source validation continuity"
        );
    }

    #[test]
    fn compact_continuity_artifact_attributes_self_upgrade_validation_source() {
        let artifact = compact_continuity_artifact();
        let self_upgrade_command =
            "rtk cargo run -q -p runner-cli -- forge-loop self-upgrade --dry-run";

        assert!(
            artifact
                .phase_validation_commands
                .get("Upgrade")
                .is_some_and(|commands| commands.contains(&self_upgrade_command.to_string())),
            "compact continuity artifact must keep self-upgrade validation under Upgrade"
        );
        assert!(
            artifact.validation_sources.iter().any(|entry| entry
                == &format!("phase=Upgrade source=required_gate_commands validation_state=pending command={self_upgrade_command}")),
            "compact continuity artifact must preserve self-upgrade phase/source validation continuity"
        );
    }

    #[test]
    fn compact_continuity_artifact_exports_research_and_upgrade_validation_commands() {
        let artifact = compact_continuity_artifact();

        for phase in ["Research", "Upgrade"] {
            let commands = artifact
                .phase_validation_commands
                .get(phase)
                .unwrap_or_else(|| panic!("compact continuity artifact missing {phase} commands"));
            assert!(
                !commands.is_empty(),
                "compact continuity artifact must keep {phase} validation commands non-empty"
            );
            assert!(
                commands.iter().all(|command| command.starts_with("rtk ")),
                "{phase} validation commands must preserve rtk shell discipline"
            );
        }
    }

    #[test]
    fn compact_continuity_artifact_exports_compact_summary_hook_events() {
        let artifact = compact_continuity_artifact();

        assert_eq!(
            artifact.compact_summary_events,
            vec!["PreCompact".to_string(), "PostCompact".to_string()]
        );
    }

    #[test]
    fn compact_continuity_artifact_preserves_research_output_contract() {
        let artifact = compact_continuity_artifact();

        for required in [
            "one-line summary",
            "source-attributed findings",
            "loop component/config inventory",
            "one recommended smallest safe self-upgrade",
            "tests required before merge",
        ] {
            assert!(
                artifact
                    .research_output_contract
                    .iter()
                    .any(|entry| entry.contains(required)),
                "compact continuity artifact missing research output contract item {required}"
            );
        }
    }

    #[test]
    fn research_output_contract_names_inventory_surfaces() {
        let contract = research_output_contract().join("\n");

        for required in [
            "config",
            "hooks",
            "rules",
            "skills",
            "custom agents/subagents",
            "permission profiles",
            "model flags",
            "GitHub Action/tool surfaces",
            "structured output schemas",
            "auto-compaction/continuity settings",
        ] {
            assert!(
                contract.contains(required),
                "research output contract missing inventory surface {required}"
            );
        }
    }

    #[test]
    fn self_upgrade_plan_exports_continuity_and_target_mining_contracts() {
        let plan = self_upgrade_plan(70);

        assert_eq!(
            plan["target_mining_audit"],
            "rtk fxrun forge-loop target-mining-audit --json"
        );
        assert_eq!(plan["compact_continuity"], "compact-continuity.json");
    }

    #[test]
    fn self_upgrade_plan_audit_commands_preserve_rtk_shell_discipline() {
        let plan = self_upgrade_plan(70);

        for field in ["components_audit", "target_mining_audit"] {
            let command = plan[field]
                .as_str()
                .unwrap_or_else(|| panic!("self-upgrade plan missing {field} command"));
            assert!(
                command.starts_with("rtk "),
                "self-upgrade plan {field} command must preserve rtk shell discipline: {command}"
            );
        }
    }

    #[test]
    fn self_upgrade_plan_audit_surfaces_are_direct_rtk_commands() {
        let plan = self_upgrade_plan(70);

        for field in [
            "components_audit",
            "target_mining_audit",
            "runner_flow_audit",
            "runner_black_factor_audit",
            "runner_ops_slo_audit",
            "runner_fleet_audit",
            "runner_queue_audit",
            "agentic_system_audit",
        ] {
            let command = plan[field]
                .as_str()
                .unwrap_or_else(|| panic!("self-upgrade plan missing {field} command"));
            assert!(
                command.starts_with("rtk fxrun forge-loop "),
                "self-upgrade plan {field} command must be directly runnable through rtk: {command}"
            );
        }
    }

    #[test]
    fn self_upgrade_plan_exports_phase_validation_state_continuity() {
        let plan = self_upgrade_plan(70);
        let phase_validation_state = plan["phase_validation_state"]
            .as_object()
            .expect("self-upgrade plan phase validation state");

        for phase in required_phases() {
            let phase = cycle_phase_label(phase);
            assert_eq!(
                phase_validation_state
                    .get(phase)
                    .and_then(serde_json::Value::as_str),
                Some("pending"),
                "self-upgrade plan missing phase validation state for {phase}"
            );
        }
    }

    #[test]
    fn self_upgrade_plan_exports_phase_next_action_continuity() {
        let plan = self_upgrade_plan(70);
        let phase_next_actions = plan["phase_next_actions"]
            .as_object()
            .expect("self-upgrade plan phase next actions");

        for phase in required_phases() {
            let phase = cycle_phase_label(phase);
            let next_action = phase_next_actions
                .get(phase)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| {
                    panic!("self-upgrade plan missing phase next action for {phase}")
                });
            assert!(
                !next_action.is_empty(),
                "self-upgrade plan phase next action must not be empty for {phase}"
            );
        }
    }

    #[test]
    fn self_upgrade_plan_exports_phase_validation_commands_continuity() {
        let plan = self_upgrade_plan(70);
        let phase_validation_commands = plan["phase_validation_commands"]
            .as_object()
            .expect("self-upgrade plan phase validation commands");

        for phase in required_phases() {
            let phase = cycle_phase_label(phase);
            let commands = phase_validation_commands
                .get(phase)
                .and_then(serde_json::Value::as_array)
                .unwrap_or_else(|| {
                    panic!("self-upgrade plan missing phase validation commands for {phase}")
                });
            assert!(
                !commands.is_empty(),
                "self-upgrade plan phase validation commands must not be empty for {phase}"
            );
            assert!(
                commands.iter().all(|command| command
                    .as_str()
                    .is_some_and(|command| command.starts_with("rtk "))),
                "self-upgrade plan phase validation commands must preserve rtk shell discipline for {phase}"
            );
        }
    }

    #[test]
    fn doctor_json_exports_required_gate_contract() {
        let report = serde_json::json!({
            "required_gate_commands": REQUIRED_GATE_COMMANDS,
        });
        let gates = report["required_gate_commands"]
            .as_array()
            .expect("gate commands");

        assert!(
            gates.iter().all(|gate| gate
                .as_str()
                .is_some_and(|command| command.starts_with("rtk "))),
            "scheduled forge-loop gate commands must preserve rtk shell discipline"
        );
        assert!(gates
            .iter()
            .any(|gate| gate == "rtk cargo audit --deny warnings"));
        assert!(gates
            .iter()
            .any(|gate| gate == "rtk cargo run -q -p runner-cli -- forge-loop docs-drift --json"));
    }

    #[test]
    fn scheduled_gate_contract_enforces_strict_component_and_target_audits() {
        assert!(
            REQUIRED_GATE_COMMANDS.contains(
                &"rtk cargo run -q -p runner-cli -- forge-loop components-audit --strict"
            ),
            "scheduled validation must include the strict component inventory audit"
        );
        assert!(
            REQUIRED_GATE_COMMANDS.contains(
                &"rtk cargo run -q -p runner-cli -- forge-loop target-mining-audit --strict"
            ),
            "scheduled validation must include the strict target-mining audit"
        );
    }

    #[test]
    fn scheduled_gate_contract_exercises_doctor_readiness_surface() {
        assert!(
            REQUIRED_GATE_COMMANDS
                .contains(&"rtk cargo run -q -p runner-cli -- forge-loop doctor --json"),
            "scheduled validation must exercise the local subscription-auth readiness surface"
        );
    }

    #[test]
    fn scheduled_gate_contract_proves_compact_continuity_artifact_generation() {
        assert!(
            REQUIRED_GATE_COMMANDS.contains(
                &"rtk cargo run -q -p runner-cli -- forge-loop run --dry-run --out /tmp/fxrun-forge-loop-gate-dry-run --goal \"scheduled subscription-auth Codex self-improvement\""
            ),
            "scheduled validation must dry-run the forge-loop runner to prove compact-continuity artifact generation"
        );
    }

    #[test]
    fn scheduled_gate_contract_exercises_evaluation_surface() {
        assert!(
            REQUIRED_GATE_COMMANDS
                .contains(&"rtk cargo run -q -p runner-cli -- forge-loop eval --fixture"),
            "scheduled validation must run a deterministic forge-loop evaluation fixture"
        );
    }

    #[test]
    fn scheduled_gate_contract_validates_generated_manifest_pairing() {
        assert!(
            REQUIRED_GATE_COMMANDS.contains(
                &"rtk cargo run -q -p runner-cli -- forge-loop eval --metrics /tmp/fxrun-forge-loop-gate-dry-run/cycle/evaluation-input.json --manifest /tmp/fxrun-forge-loop-gate-dry-run/cycle/cycle-manifest.json"
            ),
            "scheduled validation must parse the generated dry-run manifest against evaluation metrics"
        );
    }

    #[test]
    fn scheduled_gate_contract_audits_structured_output_schema() {
        assert!(
            REQUIRED_GATE_COMMANDS.contains(
                &"rtk cargo run -q -p runner-cli -- forge-loop output-schema-audit --strict"
            ),
            "scheduled validation must audit the structured Codex output schema"
        );
    }

    #[test]
    fn scheduled_gate_contract_exercises_self_upgrade_plan_surface() {
        assert!(
            REQUIRED_GATE_COMMANDS
                .contains(&"rtk cargo run -q -p runner-cli -- forge-loop self-upgrade --dry-run"),
            "scheduled validation must execute the self-upgrade plan surface before the outer publisher consumes it"
        );
    }

    #[test]
    fn publisher_shell_commands_are_routed_through_rtk() {
        let (program, args) = command_invocation("gh", vec!["pr".into(), "create".into()]);

        assert_eq!(program, "rtk");
        assert_eq!(
            args,
            vec!["gh".to_string(), "pr".to_string(), "create".to_string()]
        );
    }

    #[test]
    fn find_shell_commands_use_rtk_proxy_for_compound_predicates() {
        let (program, args) = command_invocation(
            "find",
            vec![
                ".".into(),
                "-type".into(),
                "f".into(),
                "-name".into(),
                "*.rs".into(),
                "-print".into(),
            ],
        );

        assert_eq!(program, "rtk");
        assert_eq!(
            args,
            vec![
                "proxy".to_string(),
                "find".to_string(),
                ".".to_string(),
                "-type".to_string(),
                "f".to_string(),
                "-name".to_string(),
                "*.rs".to_string(),
                "-print".to_string(),
            ]
        );
    }

    #[test]
    fn pr_create_reference_parser_accepts_rtk_prefixed_success_output() {
        let parsed = parse_pr_create_reference(
            "ok created #146 https://github.com/FlexNetOS/flexnetos_runner/pull/146\n",
        )
        .expect("parse rtk-wrapped gh create output");

        assert_eq!(
            parsed,
            "https://github.com/FlexNetOS/flexnetos_runner/pull/146"
        );
    }

    #[test]
    fn pr_create_reference_parser_accepts_plain_url_or_number() {
        assert_eq!(
            parse_pr_create_reference("https://github.com/FlexNetOS/flexnetos_runner/pull/147\n")
                .expect("plain URL"),
            "https://github.com/FlexNetOS/flexnetos_runner/pull/147"
        );
        assert_eq!(
            parse_pr_create_reference("#148\n").expect("plain PR number"),
            "148"
        );
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
        assert!(report
            .missing_components
            .contains(&"archived-hooks".to_string()));

        fs::remove_dir_all(out).ok();
    }

    #[test]
    fn components_audit_exposes_checklist_shell_discipline_drift() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let readiness = components_audit_report(root).checklist_shell_discipline;

        assert_eq!(
            readiness.checklist_path,
            ".codex/checklists/forge-loop-cycle.toml"
        );
        assert_eq!(
            readiness.checked_commands.len(),
            required_checklist_command_keys().len()
        );
        assert!(
            readiness
                .raw_command_keys
                .contains(&"component_audit".to_string()),
            "components-audit must expose raw checklist command drift: {readiness:?}"
        );
        assert!(
            !readiness.rtk_ready,
            "current checklist drift must not be reported as rtk-ready"
        );
        assert!(
            readiness
                .blockers
                .iter()
                .any(|blocker| blocker.contains("not rtk-prefixed")),
            "readiness blockers must explain shell-discipline drift: {readiness:?}"
        );
    }

    #[test]
    fn checklist_shell_discipline_blockers_include_rtk_replacements() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let readiness = components_audit_report(root).checklist_shell_discipline;

        assert!(
            readiness.blockers.iter().any(|blocker| blocker.contains(
                "component_audit is not rtk-prefixed; expected: rtk cargo run -q -p runner-cli -- forge-loop components-audit --strict"
            )),
            "checklist shell-discipline blockers should include exact RTK replacements: {readiness:?}"
        );
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
            prs_history_json: None,
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
    fn runner_ops_slo_audit_accepts_explicit_completion_watch_wakeups() {
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
              {"name":"Runner Black Factor Watch (workflow_dispatch codex_completion)","status":"completed","conclusion":"success","event":"workflow_dispatch","displayTitle":"Runner Black Factor Watch (workflow_dispatch codex_completion)","createdAt":"2026-06-27T00:32:00Z","updatedAt":"2026-06-27T00:33:00Z"},
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
            prs_history_json: None,
            min_window_hours: 1,
            max_idle_gap_minutes: 40,
            min_active_or_queued_sustain: 1,
            min_event_watch_wakeups: 2,
            max_failed_ops_runs: 0,
            min_sustain_duration_minutes: 5,
            json: true,
            strict: false,
        })
        .expect("ops slo report");

        assert!(report.burn_in_ready, "{:?}", report.missing_evidence);
        assert_eq!(report.event_watch_wakeups, 2);
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_ops_slo_audit_treats_watch_run_names_as_ops_workflow() {
        assert!(is_ops_workflow("Runner Black Factor Watch"));
        assert!(is_ops_workflow(
            "Runner Black Factor Watch (workflow_run CI)"
        ));
        assert!(is_ops_workflow("Codex Forge Loop"));
        assert!(is_ops_workflow(".github/workflows/codex-forge-loop.yml"));
        assert!(is_ops_workflow("Codex Forge Loop (workflow_dispatch)"));
        assert!(!is_ops_workflow("Runner Black Factor"));
    }

    #[test]
    fn runner_ops_slo_audit_counts_codex_failure_recovered_by_clean_pr() {
        let temp = std::env::temp_dir().join(format!(
            "fxrun-runner-ops-slo-codex-recovered-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let runs = temp.join("runs.json");
        let prs = temp.join("prs.json");
        let pr_history = temp.join("prs-history.json");
        fs::write(
            &runs,
            r#"[
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T00:00:00Z","updatedAt":"2026-06-27T00:06:00Z"},
              {"name":"Runner Black Factor Watch","status":"completed","conclusion":"success","event":"workflow_run","displayTitle":"Runner Black Factor Watch (workflow_run Runner Sustain)","headBranch":"main","createdAt":"2026-06-27T00:06:00Z","updatedAt":"2026-06-27T00:07:00Z"},
              {"name":"Codex Forge Loop","status":"completed","conclusion":"failure","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T00:10:00Z","updatedAt":"2026-06-27T00:16:00Z"},
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T00:35:00Z","updatedAt":"2026-06-27T00:41:00Z"},
              {"name":"Runner Sustain","status":"queued","conclusion":"","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T01:00:00Z","updatedAt":"2026-06-27T01:00:00Z"}
            ]"#,
        )
        .expect("runs json");
        fs::write(&prs, r#"[]"#).expect("prs json");
        fs::write(
            &pr_history,
            r#"[
              {"state":"MERGED","mergedAt":"2026-06-27T00:21:00Z","statusCheckRollup":[{"name":"Local Linux CI","status":"COMPLETED","conclusion":"SUCCESS"},{"name":"Semantic PR Title","status":"COMPLETED","conclusion":"SUCCESS"}]}
            ]"#,
        )
        .expect("prs history json");

        let report = runner_ops_slo_audit_report(&RunnerOpsSloAuditArgs {
            runs_json: runs,
            prs_json: prs,
            prs_history_json: Some(pr_history),
            min_window_hours: 1,
            max_idle_gap_minutes: 30,
            min_active_or_queued_sustain: 1,
            min_event_watch_wakeups: 1,
            max_failed_ops_runs: 0,
            min_sustain_duration_minutes: 5,
            json: true,
            strict: false,
        })
        .expect("ops slo report");

        assert!(report.burn_in_ready, "{:?}", report.missing_evidence);
        assert_eq!(report.failed_ops_runs, 0);
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_ops_slo_audit_counts_failed_ops_recovered_by_successful_replacement() {
        let temp = std::env::temp_dir().join(format!(
            "fxrun-runner-ops-slo-recovered-replacements-{}",
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
              {"name":"Runner Black Factor Watch (workflow_run CI)","status":"completed","conclusion":"failure","event":"workflow_run","headBranch":"main","createdAt":"2026-06-27T00:10:00Z","updatedAt":"2026-06-27T00:11:00Z"},
              {"name":"Runner Black Factor Watch (workflow_run CI)","status":"completed","conclusion":"success","event":"workflow_run","headBranch":"main","createdAt":"2026-06-27T00:14:00Z","updatedAt":"2026-06-27T00:15:00Z"},
              {"name":"CI","status":"completed","conclusion":"action_required","event":"pull_request","headBranch":"codex/self-upgrade","createdAt":"2026-06-27T00:20:00Z","updatedAt":"2026-06-27T00:20:00Z"},
              {"name":"CI","status":"completed","conclusion":"success","event":"pull_request","headBranch":"codex/self-upgrade","createdAt":"2026-06-27T00:23:00Z","updatedAt":"2026-06-27T00:25:00Z"},
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T00:35:00Z","updatedAt":"2026-06-27T00:41:00Z"},
              {"name":"Runner Sustain","status":"queued","conclusion":"","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T01:00:00Z","updatedAt":"2026-06-27T01:00:00Z"}
            ]"#,
        )
        .expect("runs json");
        fs::write(&prs, r#"[]"#).expect("prs json");

        let report = runner_ops_slo_audit_report(&RunnerOpsSloAuditArgs {
            runs_json: runs,
            prs_json: prs,
            prs_history_json: None,
            min_window_hours: 1,
            max_idle_gap_minutes: 30,
            min_active_or_queued_sustain: 1,
            min_event_watch_wakeups: 1,
            max_failed_ops_runs: 0,
            min_sustain_duration_minutes: 5,
            json: true,
            strict: false,
        })
        .expect("ops slo report");

        assert!(report.burn_in_ready, "{:?}", report.missing_evidence);
        assert_eq!(report.failed_ops_runs, 0);
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_ops_slo_audit_accepts_recovered_idle_gap_after_clean_rehydrate_pr() {
        let temp = std::env::temp_dir().join(format!(
            "fxrun-runner-ops-slo-recovered-idle-gap-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let runs = temp.join("runs.json");
        let prs = temp.join("prs.json");
        let pr_history = temp.join("prs-history.json");
        fs::write(
            &runs,
            r#"[
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T00:00:00Z","updatedAt":"2026-06-27T00:05:00Z"},
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T00:30:00Z","updatedAt":"2026-06-27T00:56:00Z"},
              {"name":"Runner Black Factor Watch (workflow_run CI)","status":"completed","conclusion":"success","event":"workflow_run","displayTitle":"Runner Black Factor Watch (workflow_run CI)","headBranch":"main","createdAt":"2026-06-27T00:38:00Z","updatedAt":"2026-06-27T00:39:00Z"},
              {"name":"Runner Sustain","status":"in_progress","conclusion":"","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T00:56:00Z","updatedAt":"2026-06-27T01:00:00Z"},
              {"name":"Runner Sustain","status":"queued","conclusion":"","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T01:00:00Z","updatedAt":"2026-06-27T01:00:00Z"}
            ]"#,
        )
        .expect("runs json");
        fs::write(&prs, r#"[]"#).expect("prs json");
        fs::write(
            &pr_history,
            r#"[
              {"title":"fix: rehydrate after codex completion pressure clears","state":"MERGED","mergedAt":"2026-06-27T00:32:00Z","statusCheckRollup":[{"name":"Local Linux CI","status":"COMPLETED","conclusion":"SUCCESS"},{"name":"Semantic PR Title","status":"COMPLETED","conclusion":"SUCCESS"}]}
            ]"#,
        )
        .expect("prs history json");

        let report = runner_ops_slo_audit_report(&RunnerOpsSloAuditArgs {
            runs_json: runs,
            prs_json: prs,
            prs_history_json: Some(pr_history),
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
        assert_eq!(report.max_idle_gap_minutes_observed, 25);
        assert_eq!(report.max_unrecovered_idle_gap_minutes, 0);
        assert_eq!(report.recovered_idle_gap_minutes, 25);
        assert_eq!(report.recovered_idle_gaps, 1);
        fs::remove_dir_all(temp).ok();
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
            prs_history_json: None,
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
            prs_history_json: None,
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
    fn runner_ops_slo_audit_counts_codex_growth_as_productive_runner_work() {
        let temp = std::env::temp_dir().join(format!(
            "fxrun-runner-ops-slo-codex-work-{}",
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
              {"name":"Runner Black Factor Watch","status":"completed","conclusion":"success","event":"workflow_run","headBranch":"main","createdAt":"2026-06-27T00:06:00Z","updatedAt":"2026-06-27T00:07:00Z"},
              {"name":"Codex Forge Loop","status":"completed","conclusion":"success","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T00:10:00Z","updatedAt":"2026-06-27T00:50:00Z"},
              {"name":"Codex Forge Loop","status":"in_progress","conclusion":"","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T01:00:00Z","updatedAt":"2026-06-27T01:00:00Z"}
            ]"#,
        )
        .expect("runs json");
        fs::write(&prs, r#"[]"#).expect("prs json");

        let report = runner_ops_slo_audit_report(&RunnerOpsSloAuditArgs {
            runs_json: runs,
            prs_json: prs,
            prs_history_json: None,
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
        assert_eq!(report.active_or_queued_sustain_runs, 0);
        assert_eq!(report.active_or_queued_codex_growth_runs, 1);
        assert!(report.sustain_or_growth_backlog_ready);
        assert!(report.max_idle_gap_minutes_observed <= 10);
        assert!(!report
            .missing_evidence
            .contains(&"active_or_queued_sustain_backlog"));
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_ops_slo_audit_ignores_superseded_named_watch_cancellations() {
        let temp = std::env::temp_dir().join(format!(
            "fxrun-runner-ops-slo-watch-cancel-{}",
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
              {"name":"Runner Black Factor Watch (workflow_run Semantic PR Title)","status":"completed","conclusion":"cancelled","event":"workflow_run","headBranch":"main","createdAt":"2026-06-27T00:10:00Z","updatedAt":"2026-06-27T00:10:30Z"},
              {"name":"Runner Black Factor Watch (workflow_run CI)","status":"completed","conclusion":"success","event":"workflow_run","headBranch":"main","createdAt":"2026-06-27T00:11:00Z","updatedAt":"2026-06-27T00:12:00Z"},
              {"name":"Runner Sustain","status":"completed","conclusion":"success","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T00:30:00Z","updatedAt":"2026-06-27T00:36:00Z"},
              {"name":"Runner Sustain","status":"queued","conclusion":"","event":"workflow_dispatch","headBranch":"main","createdAt":"2026-06-27T01:00:00Z","updatedAt":"2026-06-27T01:00:00Z"}
            ]"#,
        )
        .expect("runs json");
        fs::write(&prs, r#"[]"#).expect("prs json");

        let report = runner_ops_slo_audit_report(&RunnerOpsSloAuditArgs {
            runs_json: runs,
            prs_json: prs,
            prs_history_json: None,
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
            prs_history_json: None,
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
    fn runner_fleet_audit_flags_out_of_scope_repo_lane_pressure() {
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
              {"repository":"ExternalOrg/meta","workflow":"CI","run_id":"3","job":"integration","workspace":"/runner/_work/external","head_ref":"chore/remove-empty-repos","pids":[30,31]},
              {"repository":"ExternalOrg/meta","workflow":"CI","run_id":"3","job":"integration","workspace":"/runner/_work/external","head_ref":"chore/remove-empty-repos","pids":[32]}
            ]"#,
        )
        .expect("jobs json");

        let report = runner_fleet_audit_report(&RunnerFleetAuditArgs {
            expected_scope: "FlexNetOS/".to_string(),
            jobs_json: Some(jobs),
            proc_root: PathBuf::from("/proc"),
            max_out_of_scope_jobs: 0,
            json: true,
            strict: false,
        })
        .expect("fleet audit");

        assert!(!report.fleet_ready);
        assert_eq!(report.total_jobs, 3);
        assert_eq!(report.in_scope_repository_jobs, 2);
        assert_eq!(report.out_of_scope_repository_jobs, 1);
        assert_eq!(
            report.out_of_scope_repositories.get("ExternalOrg/meta"),
            Some(&1)
        );
        assert!(
            report
                .missing_evidence
                .contains(&"out_of_scope_runner_lane_pressure"),
            "{:?}",
            report.missing_evidence
        );
        let external_job = report
            .jobs
            .iter()
            .find(|job| job.repository == "ExternalOrg/meta")
            .expect("external job");
        assert_eq!(external_job.pids, vec![30, 31, 32]);
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_fleet_audit_default_scope_accepts_all_flexnetos_org_repos() {
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
              {"repository":"FlexNetOS/flexnetos_runner","workflow":"Runner Sustain","run_id":"1","job":"local-runner-sustain","workspace":"/runner/_work/flexnetos_runner","pids":[10]},
              {"repository":"FlexNetOS/meta","workflow":"CI","run_id":"2","job":"integration","workspace":"/runner/_work/meta","pids":[20]},
              {"repository":"FlexNetOS/envctl","workflow":"CI","run_id":"3","job":"test","workspace":"/runner/_work/envctl","pids":[30]}
            ]"#,
        )
        .expect("jobs json");

        let report = runner_fleet_audit_report(&RunnerFleetAuditArgs {
            expected_scope: "FlexNetOS/".to_string(),
            jobs_json: Some(jobs),
            proc_root: PathBuf::from("/proc"),
            max_out_of_scope_jobs: 0,
            json: true,
            strict: false,
        })
        .expect("fleet audit");

        assert!(report.fleet_ready, "{:?}", report.missing_evidence);
        assert_eq!(report.total_jobs, 3);
        assert_eq!(report.in_scope_repository_jobs, 3);
        assert_eq!(report.out_of_scope_repository_jobs, 0);
        assert!(report.out_of_scope_repositories.is_empty());
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_queue_audit_classifies_local_waits_and_nonlocal_queues() {
        let temp =
            std::env::temp_dir().join(format!("fxrun-runner-queue-audit-{}", std::process::id()));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let repo_jobs = temp.join("repo-jobs.json");
        fs::write(
            &repo_jobs,
            r#"[
              {
                "repository":"FlexNetOS/envctl",
                "run_id":"28341300089",
                "name":"CI",
                "run_status":"in_progress",
                "event":"pull_request",
                "displayTitle":"engine: add catalog diff and render projections",
                "headBranch":"engine/catalog-diff",
                "jobs":[
                  {"name":"test","status":"in_progress","conclusion":null,"runner_name":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","runner_group_name":"default","labels":["self-hosted","linux","x64","local","flexnetos"]}
                ]
              },
              {
                "repository":"FlexNetOS/flexnetos_runner",
                "run_id":"28341367600",
                "name":"Codex Forge Loop",
                "run_status":"queued",
                "event":"workflow_dispatch",
                "displayTitle":"Codex Forge Loop",
                "headBranch":"main",
                "jobs":[
                  {"name":"forge-loop","status":"queued","conclusion":null,"labels":["self-hosted","linux","x64","local","flexnetos"]}
                ]
              },
              {
                "repository":"FlexNetOS/meta",
                "run_id":"28341528867",
                "name":"CI",
                "run_status":"queued",
                "event":"pull_request",
                "displayTitle":"docs(recovery): prove phase 1.5 release infra",
                "headBranch":"codex/phase1-5-release-infra-proof-20260629",
                "jobs":[
                  {"name":"Clippy","status":"queued","conclusion":null,"labels":["self-hosted","linux","x64","local","flexnetos"]}
                ]
              },
              {
                "repository":"FlexNetOS/n8n",
                "run_id":"28315602550",
                "name":"Test: E2E VM Expressions Nightly",
                "run_status":"queued",
                "event":"schedule",
                "displayTitle":"Test: E2E VM Expressions Nightly",
                "headBranch":"master",
                "jobs":[
                  {"name":"e2e","status":"queued","conclusion":null,"labels":["blacksmith-4vcpu-ubuntu-2204"]}
                ]
              }
            ]"#,
        )
        .expect("repo jobs json");

        let report = runner_queue_audit_report(&RunnerQueueAuditArgs {
            repo_jobs_json: repo_jobs,
            max_queued_local_jobs: 0,
            json: true,
            strict: false,
        })
        .expect("queue audit");

        assert!(!report.queue_ready);
        assert_eq!(report.scanned_repositories, 4);
        assert_eq!(report.active_local_runner_jobs.len(), 1);
        assert_eq!(report.queued_local_runner_jobs.len(), 2);
        assert_eq!(report.nonlocal_queued_jobs.len(), 1);
        assert_eq!(
            report
                .local_runner_busy_repositories
                .get("FlexNetOS/envctl"),
            Some(&1)
        );
        assert_eq!(
            report
                .local_runner_waiting_repositories
                .get("FlexNetOS/flexnetos_runner"),
            Some(&1)
        );
        assert_eq!(
            report
                .local_runner_waiting_repositories
                .get("FlexNetOS/meta"),
            Some(&1)
        );
        assert_eq!(report.trigger_events.get("pull_request"), Some(&2));
        assert_eq!(report.trigger_events.get("workflow_dispatch"), Some(&1));
        assert_eq!(report.trigger_events.get("schedule"), Some(&1));
        assert!(report
            .missing_evidence
            .contains(&"local_runner_queue_pressure"));
        let n8n = report
            .repositories
            .iter()
            .find(|repo| repo.repository == "FlexNetOS/n8n")
            .expect("n8n summary");
        assert_eq!(n8n.nonlocal_queued_jobs, 1);
        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn runner_queue_audit_accepts_nonlocal_vendor_queue_without_local_pressure() {
        let temp = std::env::temp_dir().join(format!(
            "fxrun-runner-queue-audit-pass-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).expect("tempdir");
        let repo_jobs = temp.join("repo-jobs.json");
        fs::write(
            &repo_jobs,
            r#"[
              {
                "repository":"FlexNetOS/flexnetos_runner",
                "run_id":28341315842,
                "name":"CI",
                "run_status":"in_progress",
                "event":"push",
                "jobs":[
                  {"name":"Local Linux CI","status":"in_progress","runner_name":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","runner_group_name":"default","labels":["self-hosted","linux","x64","local","flexnetos"]}
                ]
              },
              {
                "repository":"drdave-flexnetos/chroma",
                "run_id":28318892433,
                "name":"Run (intensive) tests nightly",
                "run_status":"queued",
                "event":"schedule",
                "jobs":[
                  {"name":"intensive","status":"queued","labels":["blacksmith-8vcpu-ubuntu-2404"]}
                ]
              }
            ]"#,
        )
        .expect("repo jobs json");

        let report = runner_queue_audit_report(&RunnerQueueAuditArgs {
            repo_jobs_json: repo_jobs,
            max_queued_local_jobs: 0,
            json: true,
            strict: false,
        })
        .expect("queue audit");

        assert!(report.queue_ready, "{:?}", report.missing_evidence);
        assert_eq!(report.active_local_runner_jobs.len(), 1);
        assert!(report.queued_local_runner_jobs.is_empty());
        assert_eq!(report.nonlocal_queued_jobs.len(), 1);
        assert!(report.local_runner_waiting_repositories.is_empty());
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
            expected_scope: "FlexNetOS/flexnetos_runner".to_string(),
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
            max_out_of_scope_jobs: 0,
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
        assert!(workflow.contains("forge-loop run"));
        assert!(workflow.contains("local ChatGPT auth"));
    }

    #[test]
    fn components_audit_exposes_permission_profile_migration_readiness() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let readiness = components_audit_report(root).permission_profile_readiness;

        assert_eq!(
            readiness.mirror_default_permissions.as_deref(),
            Some("forge-loop-workspace")
        );
        assert!(
            readiness.profile_rules_present,
            "permission profile mirror must retain least-privilege parity rules"
        );
        assert!(
            readiness
                .blockers
                .iter()
                .any(|blocker| blocker.contains("sandbox_mode")),
            "readiness audit must expose the active sandbox_mode migration blocker"
        );
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
    fn action_output_schema_requires_full_loop_component_inventory() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let output_schema =
            fs::read_to_string(root.join(".github/codex/schemas/forge-loop-output.schema.json"))
                .expect("read output schema");

        for required in [
            "model_flags",
            "tool_surfaces",
            "structured_output_schemas",
            "auto_compaction_continuity_settings",
            "validation_sources",
            "phase_continuity",
        ] {
            assert!(
                output_schema.contains(required),
                "output schema component inventory missing {required}"
            );
        }
    }

    #[test]
    fn action_output_schema_records_codex_auth_mode() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let output_schema =
            fs::read_to_string(root.join(".github/codex/schemas/forge-loop-output.schema.json"))
                .expect("read output schema");

        for required in ["auth_mode", "api_key", "local_chatgpt"] {
            assert!(
                output_schema.contains(required),
                "output schema must record Codex auth mode via {required}"
            );
        }
    }

    #[test]
    fn action_output_schema_requires_subscription_auth_evidence() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let output_schema =
            fs::read_to_string(root.join(".github/codex/schemas/forge-loop-output.schema.json"))
                .expect("read output schema");

        for required in [
            "auth_evidence",
            "codex_home",
            "login_status_checked",
            "auth_json_present",
        ] {
            assert!(
                output_schema.contains(required),
                "output schema must require subscription auth evidence via {required}"
            );
        }
    }

    #[test]
    fn output_schema_audit_requires_subscription_auth_inventory_and_continuity() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");

        let report = output_schema_audit_report(root).expect("schema audit");

        assert!(report.schema_valid_json);
        assert!(
            report.structured_output_ready,
            "{:?}",
            report.missing_fields
        );
        for required in [
            "auth_mode",
            "auth_evidence",
            "codex_home",
            "component_inventory",
            "structured_output_schemas",
            "recommended_self_upgrade",
            "tests_required_before_merge",
            "auto_compact_continuity",
            "phases",
            "active_phase",
            "current_phase_index",
            "source_coverage",
            "validation_state",
            "validation_sources",
            "phase_next_actions",
            "phase_validation_state",
            "next_action",
            "phase_source_validation_next_action",
        ] {
            assert!(
                report.present_fields.contains(&required.to_string()),
                "schema audit missing {required}"
            );
        }
    }

    #[test]
    fn output_schema_audit_requires_phase_order_continuity() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");

        let report = output_schema_audit_report(root).expect("schema audit");

        assert!(
            report.present_fields.contains(&"phases".to_string()),
            "schema audit must require canonical phase order continuity: {:?}",
            report.missing_fields
        );
    }

    #[test]
    fn output_schema_audit_rejects_fields_that_are_only_documented_not_required() {
        let root = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-schema-required-audit-{}",
            std::process::id()
        ));
        let schema_dir = root.join(".github/codex/schemas");
        fs::create_dir_all(&schema_dir).expect("schema dir");
        fs::write(
            schema_dir.join("forge-loop-output.schema.json"),
            r#"{
              "type": "object",
              "required": ["summary"],
              "properties": {
                "summary": {"type": "string"},
                "auth_mode": {"type": "string"},
                "component_inventory": {
                  "type": "object",
                  "required": [],
                  "properties": {
                    "structured_output_schemas": {"type": "string"}
                  }
                },
                "auto_compact_continuity": {
                  "type": "object",
                  "required": [],
                  "properties": {
                    "next_action": {"type": "string"}
                  }
                }
              }
            }"#,
        )
        .expect("write schema");

        let report = output_schema_audit_report(&root).expect("schema audit");

        assert!(!report.structured_output_ready);
        assert!(
            report.missing_fields.contains(&"auth_mode".to_string()),
            "field mentioned only in properties must still be missing: {:?}",
            report
        );
        assert!(
            report
                .missing_fields
                .contains(&"structured_output_schemas".to_string()),
            "nested property mentioned outside a required array must still be missing: {:?}",
            report
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn output_schema_audit_rejects_empty_phase_validation_command_arrays() {
        let root = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-schema-empty-phase-commands-{}",
            std::process::id()
        ));
        let schema_dir = root.join(".github/codex/schemas");
        fs::create_dir_all(&schema_dir).expect("schema dir");
        fs::write(
            schema_dir.join("forge-loop-output.schema.json"),
            r#"{
              "type": "object",
              "required": ["summary", "auth_mode", "auth_evidence", "sources_mined", "component_inventory", "recommended_self_upgrade", "tests_required_before_merge", "verification", "auto_compact_continuity"],
              "properties": {
                "summary": {"type": "string"},
                "auth_mode": {"type": "string"},
                "auth_evidence": {
                  "type": "object",
                  "required": ["codex_home", "login_status_checked", "auth_json_present"],
                  "properties": {
                    "codex_home": {"type": "string"},
                    "login_status_checked": {"type": "boolean"},
                    "auth_json_present": {"type": "boolean"}
                  }
                },
                "sources_mined": {"type": "array"},
                "component_inventory": {
                  "type": "object",
                  "required": ["config", "hooks", "rules", "skills", "agents", "permissions", "github_action", "model_flags", "tool_surfaces", "structured_output_schemas", "auto_compaction_continuity_settings"],
                  "properties": {
                    "config": {"type": "string"},
                    "hooks": {"type": "string"},
                    "rules": {"type": "string"},
                    "skills": {"type": "string"},
                    "agents": {"type": "string"},
                    "permissions": {"type": "string"},
                    "github_action": {"type": "string"},
                    "model_flags": {"type": "string"},
                    "tool_surfaces": {"type": "string"},
                    "structured_output_schemas": {"type": "string"},
                    "auto_compaction_continuity_settings": {"type": "string"}
                  }
                },
                "recommended_self_upgrade": {"type": "string"},
                "tests_required_before_merge": {"type": "array"},
                "verification": {"type": "array"},
                "auto_compact_continuity": {
                  "type": "object",
                  "required": ["enabled", "compact_prompt", "preserved_state", "active_phase", "current_phase_index", "source_coverage", "validation_state", "validation_terminal_state", "validation_sources", "phase_continuity", "phase_next_actions", "phase_validation_commands", "phase_validation_state", "next_action", "phase_source_validation_next_action"],
                  "properties": {
                    "enabled": {"type": "boolean"},
                    "compact_prompt": {"type": "string"},
                    "preserved_state": {"type": "array"},
                    "active_phase": {"type": "string"},
                    "current_phase_index": {"type": "integer"},
                    "source_coverage": {"type": "array"},
                    "validation_state": {"type": "array"},
                    "validation_terminal_state": {"type": "array"},
                    "validation_sources": {"type": "array"},
                    "phase_continuity": {"type": "array"},
                    "phase_next_actions": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]
                    },
                    "phase_validation_commands": {
                      "type": "object",
                      "required": ["Gate", "Evaluate"],
                      "properties": {
                        "Gate": {"type": "array", "items": {"type": "string"}},
                        "Evaluate": {"type": "array", "items": {"type": "string"}}
                      }
                    },
                    "phase_validation_state": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]
                    },
                    "next_action": {"type": "string"},
                    "phase_source_validation_next_action": {"type": "string"}
                  }
                }
              }
            }"#,
        )
        .expect("write schema");

        let report = output_schema_audit_report(&root).expect("schema audit");

        assert!(!report.structured_output_ready);
        assert!(
            report
                .missing_fields
                .contains(&"phase_validation_commands.Gate.minItems".to_string()),
            "Gate command array must require at least one command: {:?}",
            report
        );
        assert!(
            report
                .missing_fields
                .contains(&"phase_validation_commands.Evaluate.minItems".to_string()),
            "Evaluate command array must require at least one command: {:?}",
            report
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn output_schema_audit_rejects_missing_research_and_upgrade_phase_validation_commands() {
        let root = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-schema-research-upgrade-commands-{}",
            std::process::id()
        ));
        let schema_dir = root.join(".github/codex/schemas");
        fs::create_dir_all(&schema_dir).expect("schema dir");
        fs::write(
            schema_dir.join("forge-loop-output.schema.json"),
            r#"{
              "type": "object",
              "required": ["summary", "auth_mode", "auth_evidence", "sources_mined", "component_inventory", "recommended_self_upgrade", "tests_required_before_merge", "verification", "auto_compact_continuity"],
              "properties": {
                "summary": {"type": "string"},
                "auth_mode": {"type": "string"},
                "auth_evidence": {
                  "type": "object",
                  "required": ["codex_home", "login_status_checked", "auth_json_present"],
                  "properties": {
                    "codex_home": {"type": "string"},
                    "login_status_checked": {"type": "boolean"},
                    "auth_json_present": {"type": "boolean"}
                  }
                },
                "sources_mined": {"type": "array"},
                "component_inventory": {
                  "type": "object",
                  "required": ["config", "hooks", "rules", "skills", "agents", "permissions", "github_action", "model_flags", "tool_surfaces", "structured_output_schemas", "auto_compaction_continuity_settings"],
                  "properties": {
                    "config": {"type": "string"},
                    "hooks": {"type": "string"},
                    "rules": {"type": "string"},
                    "skills": {"type": "string"},
                    "agents": {"type": "string"},
                    "permissions": {"type": "string"},
                    "github_action": {"type": "string"},
                    "model_flags": {"type": "string"},
                    "tool_surfaces": {"type": "string"},
                    "structured_output_schemas": {"type": "string"},
                    "auto_compaction_continuity_settings": {"type": "string"}
                  }
                },
                "recommended_self_upgrade": {"type": "string"},
                "tests_required_before_merge": {"type": "array"},
                "verification": {"type": "array"},
                "auto_compact_continuity": {
                  "type": "object",
                  "required": ["enabled", "compact_prompt", "preserved_state", "active_phase", "current_phase_index", "source_coverage", "validation_state", "validation_terminal_state", "validation_sources", "phase_continuity", "phase_next_actions", "phase_validation_commands", "phase_validation_state", "next_action", "phase_source_validation_next_action"],
                  "properties": {
                    "enabled": {"type": "boolean"},
                    "compact_prompt": {"type": "string"},
                    "preserved_state": {"type": "array"},
                    "active_phase": {"type": "string"},
                    "current_phase_index": {"type": "integer"},
                    "source_coverage": {"type": "array"},
                    "validation_state": {"type": "array"},
                    "validation_terminal_state": {"type": "array"},
                    "validation_sources": {"type": "array"},
                    "phase_continuity": {"type": "array"},
                    "phase_next_actions": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]
                    },
                    "phase_validation_commands": {
                      "type": "object",
                      "required": ["Gate", "Evaluate"],
                      "properties": {
                        "Gate": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                        "Evaluate": {"type": "array", "minItems": 1, "items": {"type": "string"}}
                      }
                    },
                    "phase_validation_state": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"],
                      "properties": {
                        "Red": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Implement": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Gate": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Evaluate": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Research": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Upgrade": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]}
                      }
                    },
                    "next_action": {"type": "string"},
                    "phase_source_validation_next_action": {"type": "string"}
                  }
                }
              }
            }"#,
        )
        .expect("write schema");

        let report = output_schema_audit_report(&root).expect("schema audit");

        assert!(!report.structured_output_ready);
        for missing in [
            "phase_validation_commands.Research.minItems",
            "phase_validation_commands.Upgrade.minItems",
        ] {
            assert!(
                report.missing_fields.contains(&missing.to_string()),
                "phase validation commands must require {missing}: {:?}",
                report
            );
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn action_output_schema_omits_responses_api_rejected_unique_items_keyword() {
        fn contains_schema_key(value: &serde_json::Value, key: &str) -> bool {
            match value {
                serde_json::Value::Object(map) => map
                    .iter()
                    .any(|(candidate, child)| candidate == key || contains_schema_key(child, key)),
                serde_json::Value::Array(items) => {
                    items.iter().any(|child| contains_schema_key(child, key))
                }
                _ => false,
            }
        }

        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let output_schema =
            fs::read_to_string(root.join(".github/codex/schemas/forge-loop-output.schema.json"))
                .expect("read output schema");
        let parsed: serde_json::Value =
            serde_json::from_str(&output_schema).expect("parse output schema");

        assert!(
            !contains_schema_key(&parsed, "uniqueItems"),
            "Codex Responses structured output rejects uniqueItems; schema must avoid it"
        );
    }

    #[test]
    fn output_schema_audit_rejects_unconstrained_phase_validation_command_items() {
        let root = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-schema-phase-command-pattern-{}",
            std::process::id()
        ));
        let schema_dir = root.join(".github/codex/schemas");
        fs::create_dir_all(&schema_dir).expect("schema dir");
        fs::write(
            schema_dir.join("forge-loop-output.schema.json"),
            r#"{
              "type": "object",
              "required": ["summary", "auth_mode", "auth_evidence", "sources_mined", "component_inventory", "recommended_self_upgrade", "tests_required_before_merge", "verification", "auto_compact_continuity"],
              "properties": {
                "summary": {"type": "string"},
                "auth_mode": {"type": "string"},
                "auth_evidence": {
                  "type": "object",
                  "required": ["codex_home", "login_status_checked", "auth_json_present"],
                  "properties": {
                    "codex_home": {"type": "string"},
                    "login_status_checked": {"type": "boolean"},
                    "auth_json_present": {"type": "boolean"}
                  }
                },
                "sources_mined": {"type": "array"},
                "component_inventory": {
                  "type": "object",
                  "required": ["config", "hooks", "rules", "skills", "agents", "permissions", "github_action", "model_flags", "tool_surfaces", "structured_output_schemas", "auto_compaction_continuity_settings"],
                  "properties": {
                    "config": {"type": "string"},
                    "hooks": {"type": "string"},
                    "rules": {"type": "string"},
                    "skills": {"type": "string"},
                    "agents": {"type": "string"},
                    "permissions": {"type": "string"},
                    "github_action": {"type": "string"},
                    "model_flags": {"type": "string"},
                    "tool_surfaces": {"type": "string"},
                    "structured_output_schemas": {"type": "string"},
                    "auto_compaction_continuity_settings": {"type": "string"}
                  }
                },
                "recommended_self_upgrade": {"type": "string"},
                "tests_required_before_merge": {"type": "array"},
                "verification": {"type": "array"},
                "auto_compact_continuity": {
                  "type": "object",
                  "required": ["enabled", "compact_prompt", "preserved_state", "phases", "active_phase", "current_phase_index", "source_coverage", "validation_state", "validation_terminal_state", "validation_sources", "phase_continuity", "phase_next_actions", "phase_validation_commands", "phase_validation_state", "next_action", "phase_source_validation_next_action"],
                  "properties": {
                    "enabled": {"type": "boolean"},
                    "compact_prompt": {"type": "string"},
                    "preserved_state": {"type": "array"},
                    "phases": {"type": "array", "minItems": 6, "items": {"type": "string", "enum": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]}},
                    "active_phase": {"type": "string", "enum": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]},
                    "current_phase_index": {"type": "integer", "minimum": 0},
                    "source_coverage": {"type": "array"},
                    "validation_state": {"type": "array"},
                    "validation_terminal_state": {"type": "array"},
                    "validation_sources": {"type": "array"},
                    "phase_continuity": {"type": "array"},
                    "phase_next_actions": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]
                    },
                    "phase_validation_commands": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"],
                      "properties": {
                        "Red": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                        "Implement": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                        "Gate": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                        "Evaluate": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                        "Research": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                        "Upgrade": {"type": "array", "minItems": 1, "items": {"type": "string"}}
                      }
                    },
                    "phase_validation_state": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"],
                      "properties": {
                        "Red": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Implement": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Gate": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Evaluate": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Research": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Upgrade": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]}
                      }
                    },
                    "next_action": {"type": "string"},
                    "phase_source_validation_next_action": {"type": "string"}
                  }
                }
              }
            }"#,
        )
        .expect("write schema");

        let report = output_schema_audit_report(&root).expect("schema audit");

        assert!(!report.structured_output_ready);
        assert!(
            report
                .missing_fields
                .contains(&"phase_validation_commands.Red.items.pattern".to_string()),
            "phase validation command schema must require rtk-prefixed commands: {:?}",
            report
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn output_schema_audit_rejects_unconstrained_phase_validation_state_values() {
        let root = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-schema-phase-state-enum-{}",
            std::process::id()
        ));
        let schema_dir = root.join(".github/codex/schemas");
        fs::create_dir_all(&schema_dir).expect("schema dir");
        fs::write(
            schema_dir.join("forge-loop-output.schema.json"),
            r#"{
              "type": "object",
              "required": ["summary", "auth_mode", "auth_evidence", "sources_mined", "component_inventory", "recommended_self_upgrade", "tests_required_before_merge", "verification", "auto_compact_continuity"],
              "properties": {
                "summary": {"type": "string"},
                "auth_mode": {"type": "string"},
                "auth_evidence": {
                  "type": "object",
                  "required": ["codex_home", "login_status_checked", "auth_json_present"],
                  "properties": {
                    "codex_home": {"type": "string"},
                    "login_status_checked": {"type": "boolean"},
                    "auth_json_present": {"type": "boolean"}
                  }
                },
                "sources_mined": {"type": "array"},
                "component_inventory": {
                  "type": "object",
                  "required": ["config", "hooks", "rules", "skills", "agents", "permissions", "github_action", "model_flags", "tool_surfaces", "structured_output_schemas", "auto_compaction_continuity_settings"],
                  "properties": {
                    "config": {"type": "string"},
                    "hooks": {"type": "string"},
                    "rules": {"type": "string"},
                    "skills": {"type": "string"},
                    "agents": {"type": "string"},
                    "permissions": {"type": "string"},
                    "github_action": {"type": "string"},
                    "model_flags": {"type": "string"},
                    "tool_surfaces": {"type": "string"},
                    "structured_output_schemas": {"type": "string"},
                    "auto_compaction_continuity_settings": {"type": "string"}
                  }
                },
                "recommended_self_upgrade": {"type": "string"},
                "tests_required_before_merge": {"type": "array"},
                "verification": {"type": "array"},
                "auto_compact_continuity": {
                  "type": "object",
                  "required": ["enabled", "compact_prompt", "preserved_state", "active_phase", "current_phase_index", "source_coverage", "validation_state", "validation_terminal_state", "validation_sources", "phase_continuity", "phase_next_actions", "phase_validation_commands", "phase_validation_state", "next_action", "phase_source_validation_next_action"],
                  "properties": {
                    "enabled": {"type": "boolean"},
                    "compact_prompt": {"type": "string"},
                    "preserved_state": {"type": "array"},
                    "active_phase": {"type": "string"},
                    "current_phase_index": {"type": "integer"},
                    "source_coverage": {"type": "array"},
                    "validation_state": {"type": "array"},
                    "validation_terminal_state": {"type": "array"},
                    "validation_sources": {"type": "array"},
                    "phase_continuity": {"type": "array"},
                    "phase_next_actions": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]
                    },
                    "phase_validation_commands": {
                      "type": "object",
                      "required": ["Gate", "Evaluate"],
                      "properties": {
                        "Gate": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                        "Evaluate": {"type": "array", "minItems": 1, "items": {"type": "string"}}
                      }
                    },
                    "phase_validation_state": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"],
                      "properties": {
                        "Red": {"type": "string"},
                        "Implement": {"type": "string"},
                        "Gate": {"type": "string"},
                        "Evaluate": {"type": "string"},
                        "Research": {"type": "string"},
                        "Upgrade": {"type": "string"}
                      }
                    },
                    "next_action": {"type": "string"},
                    "phase_source_validation_next_action": {"type": "string"}
                  }
                }
              }
            }"#,
        )
        .expect("write schema");

        let report = output_schema_audit_report(&root).expect("schema audit");

        assert!(!report.structured_output_ready);
        assert!(
            report
                .missing_fields
                .contains(&"phase_validation_state.Red.enum".to_string()),
            "phase validation states must be constrained to known lifecycle statuses: {:?}",
            report
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn output_schema_audit_rejects_unconstrained_active_phase_values() {
        let root = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-schema-active-phase-enum-{}",
            std::process::id()
        ));
        let schema_dir = root.join(".github/codex/schemas");
        fs::create_dir_all(&schema_dir).expect("schema dir");
        fs::write(
            schema_dir.join("forge-loop-output.schema.json"),
            r#"{
              "type": "object",
              "required": ["summary", "auth_mode", "auth_evidence", "sources_mined", "component_inventory", "recommended_self_upgrade", "tests_required_before_merge", "verification", "auto_compact_continuity"],
              "properties": {
                "summary": {"type": "string"},
                "auth_mode": {"type": "string"},
                "auth_evidence": {
                  "type": "object",
                  "required": ["codex_home", "login_status_checked", "auth_json_present"],
                  "properties": {
                    "codex_home": {"type": "string"},
                    "login_status_checked": {"type": "boolean"},
                    "auth_json_present": {"type": "boolean"}
                  }
                },
                "sources_mined": {"type": "array"},
                "component_inventory": {
                  "type": "object",
                  "required": ["config", "hooks", "rules", "skills", "agents", "permissions", "github_action", "model_flags", "tool_surfaces", "structured_output_schemas", "auto_compaction_continuity_settings"],
                  "properties": {
                    "config": {"type": "string"},
                    "hooks": {"type": "string"},
                    "rules": {"type": "string"},
                    "skills": {"type": "string"},
                    "agents": {"type": "string"},
                    "permissions": {"type": "string"},
                    "github_action": {"type": "string"},
                    "model_flags": {"type": "string"},
                    "tool_surfaces": {"type": "string"},
                    "structured_output_schemas": {"type": "string"},
                    "auto_compaction_continuity_settings": {"type": "string"}
                  }
                },
                "recommended_self_upgrade": {"type": "string"},
                "tests_required_before_merge": {"type": "array"},
                "verification": {"type": "array"},
                "auto_compact_continuity": {
                  "type": "object",
                  "required": ["enabled", "compact_prompt", "preserved_state", "phases", "active_phase", "current_phase_index", "source_coverage", "validation_state", "validation_terminal_state", "validation_sources", "phase_continuity", "phase_next_actions", "phase_validation_commands", "phase_validation_state", "next_action", "phase_source_validation_next_action"],
                  "properties": {
                    "enabled": {"type": "boolean"},
                    "compact_prompt": {"type": "string"},
                    "preserved_state": {"type": "array"},
                    "phases": {"type": "array", "minItems": 6, "items": {"type": "string", "enum": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]}},
                    "active_phase": {"type": "string"},
                    "current_phase_index": {"type": "integer"},
                    "source_coverage": {"type": "array"},
                    "validation_state": {"type": "array"},
                    "validation_terminal_state": {"type": "array"},
                    "validation_sources": {"type": "array"},
                    "phase_continuity": {"type": "array"},
                    "phase_next_actions": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]
                    },
                    "phase_validation_commands": {
                      "type": "object",
                      "required": ["Gate", "Evaluate", "Research", "Upgrade"],
                      "properties": {
                        "Gate": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                        "Evaluate": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                        "Research": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                        "Upgrade": {"type": "array", "minItems": 1, "items": {"type": "string"}}
                      }
                    },
                    "phase_validation_state": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"],
                      "properties": {
                        "Red": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Implement": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Gate": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Evaluate": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Research": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Upgrade": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]}
                      }
                    },
                    "next_action": {"type": "string"},
                    "phase_source_validation_next_action": {"type": "string"}
                  }
                }
              }
            }"#,
        )
        .expect("write schema");

        let report = output_schema_audit_report(&root).expect("schema audit");

        assert!(!report.structured_output_ready);
        assert!(
            report
                .missing_fields
                .contains(&"active_phase.enum".to_string()),
            "active_phase must be constrained to the canonical phase names: {:?}",
            report
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn output_schema_audit_rejects_unbounded_phase_order_and_index_continuity() {
        let root = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-schema-phase-order-index-{}",
            std::process::id()
        ));
        let schema_dir = root.join(".github/codex/schemas");
        fs::create_dir_all(&schema_dir).expect("schema dir");
        fs::write(
            schema_dir.join("forge-loop-output.schema.json"),
            r#"{
              "type": "object",
              "required": ["summary", "auth_mode", "auth_evidence", "sources_mined", "component_inventory", "recommended_self_upgrade", "tests_required_before_merge", "verification", "auto_compact_continuity"],
              "properties": {
                "summary": {"type": "string"},
                "auth_mode": {"type": "string"},
                "auth_evidence": {
                  "type": "object",
                  "required": ["codex_home", "login_status_checked", "auth_json_present"],
                  "properties": {
                    "codex_home": {"type": "string"},
                    "login_status_checked": {"type": "boolean"},
                    "auth_json_present": {"type": "boolean"}
                  }
                },
                "sources_mined": {"type": "array"},
                "component_inventory": {
                  "type": "object",
                  "required": ["config", "hooks", "rules", "skills", "agents", "permissions", "github_action", "model_flags", "tool_surfaces", "structured_output_schemas", "auto_compaction_continuity_settings"],
                  "properties": {
                    "config": {"type": "string"},
                    "hooks": {"type": "string"},
                    "rules": {"type": "string"},
                    "skills": {"type": "string"},
                    "agents": {"type": "string"},
                    "permissions": {"type": "string"},
                    "github_action": {"type": "string"},
                    "model_flags": {"type": "string"},
                    "tool_surfaces": {"type": "string"},
                    "structured_output_schemas": {"type": "string"},
                    "auto_compaction_continuity_settings": {"type": "string"}
                  }
                },
                "recommended_self_upgrade": {"type": "string"},
                "tests_required_before_merge": {"type": "array"},
                "verification": {"type": "array"},
                "auto_compact_continuity": {
                  "type": "object",
                  "required": ["enabled", "compact_prompt", "preserved_state", "phases", "active_phase", "current_phase_index", "source_coverage", "validation_state", "validation_terminal_state", "validation_sources", "phase_continuity", "phase_next_actions", "phase_validation_commands", "phase_validation_state", "next_action", "phase_source_validation_next_action"],
                  "properties": {
                    "enabled": {"type": "boolean"},
                    "compact_prompt": {"type": "string"},
                    "preserved_state": {"type": "array"},
                    "phases": {"type": "array", "items": {"type": "string"}},
                    "active_phase": {"type": "string", "enum": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]},
                    "current_phase_index": {"type": "integer"},
                    "source_coverage": {"type": "array"},
                    "validation_state": {"type": "array"},
                    "validation_terminal_state": {"type": "array"},
                    "validation_sources": {"type": "array"},
                    "phase_continuity": {"type": "array"},
                    "phase_next_actions": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]
                    },
                    "phase_validation_commands": {
                      "type": "object",
                      "required": ["Gate", "Evaluate", "Research", "Upgrade"],
                      "properties": {
                        "Gate": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                        "Evaluate": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                        "Research": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                        "Upgrade": {"type": "array", "minItems": 1, "items": {"type": "string"}}
                      }
                    },
                    "phase_validation_state": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"],
                      "properties": {
                        "Red": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Implement": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Gate": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Evaluate": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Research": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Upgrade": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]}
                      }
                    },
                    "next_action": {"type": "string"},
                    "phase_source_validation_next_action": {"type": "string"}
                  }
                }
              }
            }"#,
        )
        .expect("write schema");

        let report = output_schema_audit_report(&root).expect("schema audit");

        assert!(!report.structured_output_ready);
        for missing in [
            "phases.minItems",
            "phases.items.enum",
            "current_phase_index.minimum",
        ] {
            assert!(
                report.missing_fields.contains(&missing.to_string()),
                "schema audit must reject missing {missing}: {:?}",
                report
            );
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn output_schema_audit_rejects_extra_phase_order_continuity() {
        let root = std::env::temp_dir().join(format!(
            "fxrun-forge-loop-schema-phase-cardinality-{}",
            std::process::id()
        ));
        let schema_dir = root.join(".github/codex/schemas");
        fs::create_dir_all(&schema_dir).expect("schema dir");
        fs::write(
            schema_dir.join("forge-loop-output.schema.json"),
            r#"{
              "type": "object",
              "required": ["summary", "auth_mode", "auth_evidence", "sources_mined", "component_inventory", "recommended_self_upgrade", "tests_required_before_merge", "verification", "auto_compact_continuity"],
              "properties": {
                "summary": {"type": "string"},
                "auth_mode": {"type": "string"},
                "auth_evidence": {
                  "type": "object",
                  "required": ["codex_home", "login_status_checked", "auth_json_present"],
                  "properties": {
                    "codex_home": {"type": "string"},
                    "login_status_checked": {"type": "boolean"},
                    "auth_json_present": {"type": "boolean"}
                  }
                },
                "sources_mined": {"type": "array"},
                "component_inventory": {
                  "type": "object",
                  "required": ["config", "hooks", "rules", "skills", "agents", "permissions", "github_action", "model_flags", "tool_surfaces", "structured_output_schemas", "auto_compaction_continuity_settings"],
                  "properties": {
                    "config": {"type": "string"},
                    "hooks": {"type": "string"},
                    "rules": {"type": "string"},
                    "skills": {"type": "string"},
                    "agents": {"type": "string"},
                    "permissions": {"type": "string"},
                    "github_action": {"type": "string"},
                    "model_flags": {"type": "string"},
                    "tool_surfaces": {"type": "string"},
                    "structured_output_schemas": {"type": "string"},
                    "auto_compaction_continuity_settings": {"type": "string"}
                  }
                },
                "recommended_self_upgrade": {"type": "string"},
                "tests_required_before_merge": {"type": "array"},
                "verification": {"type": "array"},
                "auto_compact_continuity": {
                  "type": "object",
                  "required": ["enabled", "compact_prompt", "preserved_state", "phases", "active_phase", "current_phase_index", "source_coverage", "validation_state", "validation_terminal_state", "validation_sources", "phase_continuity", "phase_next_actions", "phase_validation_commands", "phase_validation_state", "next_action", "phase_source_validation_next_action"],
                  "properties": {
                    "enabled": {"type": "boolean"},
                    "compact_prompt": {"type": "string"},
                    "preserved_state": {"type": "array"},
                    "phases": {"type": "array", "minItems": 6, "items": {"type": "string", "enum": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]}},
                    "active_phase": {"type": "string", "enum": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]},
                    "current_phase_index": {"type": "integer", "minimum": 0},
                    "source_coverage": {"type": "array"},
                    "validation_state": {"type": "array"},
                    "validation_terminal_state": {"type": "array"},
                    "validation_sources": {"type": "array"},
                    "phase_continuity": {"type": "array"},
                    "phase_next_actions": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"]
                    },
                    "phase_validation_commands": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"],
                      "properties": {
                        "Red": {"type": "array", "minItems": 1, "items": {"type": "string", "pattern": "^rtk "}},
                        "Implement": {"type": "array", "minItems": 1, "items": {"type": "string", "pattern": "^rtk "}},
                        "Gate": {"type": "array", "minItems": 1, "items": {"type": "string", "pattern": "^rtk "}},
                        "Evaluate": {"type": "array", "minItems": 1, "items": {"type": "string", "pattern": "^rtk "}},
                        "Research": {"type": "array", "minItems": 1, "items": {"type": "string", "pattern": "^rtk "}},
                        "Upgrade": {"type": "array", "minItems": 1, "items": {"type": "string", "pattern": "^rtk "}}
                      }
                    },
                    "phase_validation_state": {
                      "type": "object",
                      "required": ["Red", "Implement", "Gate", "Evaluate", "Research", "Upgrade"],
                      "properties": {
                        "Red": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Implement": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Gate": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Evaluate": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Research": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]},
                        "Upgrade": {"type": "string", "enum": ["pending", "in_progress", "passed", "failed"]}
                      }
                    },
                    "next_action": {"type": "string"},
                    "phase_source_validation_next_action": {"type": "string"}
                  }
                }
              }
            }"#,
        )
        .expect("write schema");

        let report = output_schema_audit_report(&root).expect("schema audit");

        assert!(!report.structured_output_ready);
        let missing = "phases.maxItems";
        assert!(
            report.missing_fields.contains(&missing.to_string()),
            "schema audit must reject missing {missing}: {:?}",
            report
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn compact_continuity_artifact_covers_full_research_source_matrix() {
        let continuity = compact_continuity_artifact();
        for source in research_sources() {
            assert!(
                continuity
                    .source_coverage
                    .iter()
                    .any(|entry| entry.contains(source.id) && entry.contains(source.url)),
                "compact continuity artifact missing research source {}",
                source.id
            );
        }
    }

    #[test]
    fn target_mining_audit_covers_full_research_source_matrix() {
        let audited_urls = expected_target_mining_targets()
            .into_iter()
            .map(|target| target.url)
            .collect::<Vec<_>>();

        for source in research_sources() {
            if source.id == "kclaw0-upgrade-ledger" {
                continue;
            }
            assert!(
                audited_urls.contains(&source.url),
                "target-mining audit missing research source {}: {}",
                source.id,
                source.url
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
        let hooks = fs::read_to_string(
            root.join(".codex/archive/lifecycle-hooks-20260703T024950Z/hooks.json.md"),
        )
        .expect("read archived hooks");

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
            assert!(hooks.contains(script), "archived hooks missing {script}");
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
    fn research_prompt_requires_complete_source_attributed_output_shape() {
        let prompt = research_prompt("subscription-auth reliability", &research_sources());

        for required in [
            "github.com/openai/codex",
            "developers.openai.com/codex/config-advanced",
            "developers.openai.com/codex/github-action",
            "developers.openai.com/codex/permissions",
            "developers.openai.com/codex/subagents",
            "RoggeOhta/awesome-codex-cli",
            "Yeachan-Heo/oh-my-codex",
            "crates.io",
            "drdave-flexnetos/kclaw0",
            "docs/kclaw0-upgrade-ledger.md",
            "one-line summary",
            "source-attributed findings",
            "loop component/config inventory",
            "one recommended smallest safe self-upgrade",
            "tests required before merge",
        ] {
            assert!(
                prompt.contains(required),
                "research prompt missing {required}"
            );
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
        assert!(
            !workflow.contains("      GH_TOKEN: ${{ github.token }}
      MODEL_INPUT:"),
            "publisher job env must not inherit the app GITHUB_TOKEN or bot-created PR checks stay detached"
        );

        for required in [
            "runs-on: [self-hosted, linux, x64, local, flexnetos]",
            "prompt_file:",
            "model:",
            "effort:",
            "codex-forge-loop-output.md",
            "CODEX_HOME:",
            "login status",
            "no outer codex exec sandbox",
            "FXRUN_CODEX",
            "cargo run -q -p runner-cli -- forge-loop run",
            "tee codex-forge-loop-output.md",
            "GH_CONFIG_DIR:",
            "persist-credentials: false",
            "Configure local GitHub auth for publisher",
            "unset GH_TOKEN GITHUB_TOKEN",
            "gh auth setup-git --hostname github.com",
            "git remote set-url origin",
            "actions: write",
            "PROMPT_FILE_INPUT:",
            "invalid prompt_file input",
            "Rehydrate sustain/watch after Codex completion",
            "codex-completion-rehydrate.env",
            "MIN_SUSTAIN_BACKLOG: '4'",
            "POST_CODEX_REHYDRATE_RETRY_SECONDS: '30'",
            "POST_CODEX_REHYDRATE_RETRY_ATTEMPTS: '10'",
            "Codex completion waiting",
            "rehydrate_pressure_after=",
            "dispatching Runner Sustain from Codex completion lane",
            "gh workflow run runner-black-factor-watch.yml --ref main -f trigger_source=codex_completion",
            "gh workflow run agentic-system-watch.yml --ref main -f trigger_source=codex_completion",
        ] {
            assert!(workflow.contains(required), "workflow missing {required}");
        }
    }

    #[test]
    fn scheduled_codex_growth_uses_subscription_auth_only() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let workflow = fs::read_to_string(root.join(".github/workflows/codex-forge-loop.yml"))
            .expect("read Codex workflow");
        let agentic_watch =
            fs::read_to_string(root.join(".github/workflows/agentic-system-watch.yml"))
                .expect("read agentic watch workflow");
        let runner_target =
            fs::read_to_string(root.join("docs/forge-loop/kclaw0-runner-flow-target.md"))
                .expect("read runner flow target");
        let agentic_proof =
            fs::read_to_string(root.join("docs/forge-loop/agentic-system-proof.md"))
                .expect("read agentic system proof");

        for (label, text) in [
            ("codex workflow", workflow.as_str()),
            ("agentic watch workflow", agentic_watch.as_str()),
            ("runner flow target", runner_target.as_str()),
            ("agentic system proof", agentic_proof.as_str()),
        ] {
            assert!(
                !text.contains("OPENAI_API_KEY"),
                "{label} must not default scheduled Codex growth to OpenAI API-key auth"
            );
            assert!(
                !text.contains("openai-api-key"),
                "{label} must not wire Codex Action API-key auth"
            );
        }
        assert!(workflow.contains("Run Codex forge-loop prompt with local ChatGPT auth"));
        assert!(workflow.contains("CODEX_HOME:"));
        assert!(workflow.contains("FXRUN_CODEX"));
    }

    #[test]
    fn scheduled_forge_loop_prompt_leaves_pr_publication_to_outer_engine() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");

        let prompt_path = ".github/codex/prompts/forge-loop.md";
        let prompt = fs::read_to_string(root.join(prompt_path)).expect("read forge-loop prompt");
        assert!(
            prompt.contains("leave the intended repository changes in the working tree"),
            "{prompt_path} must preserve the inner/outer Codex publication handoff"
        );
        assert!(
            prompt.contains("do not run git commit, git push, or gh pr from inside Codex"),
            "{prompt_path} must forbid nested Codex from publishing repository changes"
        );
        assert!(
            !prompt.contains("Commit, push, open a PR"),
            "{prompt_path} must not tell nested Codex to publish its own PR"
        );
    }

    #[test]
    fn scheduled_forge_loop_prompt_binds_single_self_upgrade_cycle() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");

        let prompt_path = ".github/codex/prompts/forge-loop.md";
        let prompt = fs::read_to_string(root.join(prompt_path)).expect("read forge-loop prompt");

        assert!(
            prompt.contains("Do not start another cycle."),
            "{prompt_path} must prevent recursive scheduled self-improvement cycles"
        );
        assert!(
            prompt.contains("PR title 'chore: forge loop self-upgrade'"),
            "{prompt_path} must preserve the outer forge-loop PR title contract"
        );
    }

    #[test]
    fn scheduled_forge_loop_shell_commands_use_rtk_wrapper() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");

        let prompt_path = ".github/codex/prompts/forge-loop.md";
        let prompt = fs::read_to_string(root.join(prompt_path)).expect("read forge-loop prompt");

        assert!(
            prompt.contains("rtk fxrun forge-loop run --goal"),
            "{prompt_path} command must preserve RTK shell discipline"
        );
        for command in REQUIRED_GATE_COMMANDS {
            assert!(
                command.starts_with("rtk "),
                "required gate command must start with rtk: {command}"
            );
        }
    }

    #[test]
    fn scheduled_codex_growth_invokes_forge_loop_through_rtk_wrapper() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let workflow = fs::read_to_string(root.join(".github/workflows/codex-forge-loop.yml"))
            .expect("read Codex workflow");

        assert!(
            workflow.contains("rtk cargo run -q -p runner-cli -- forge-loop run"),
            "scheduled Codex growth must route the Rust forge-loop invocation through rtk"
        );
    }

    #[test]
    fn action_created_pr_required_checks_are_dispatchable() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let codex_workflow =
            fs::read_to_string(root.join(".github/workflows/codex-forge-loop.yml"))
                .expect("read Codex workflow");
        let ci_workflow =
            fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("read CI workflow");
        let semantic_workflow =
            fs::read_to_string(root.join(".github/workflows/semantic-pr-title.yml"))
                .expect("read semantic title workflow");

        assert_eq!(
            REQUIRED_CHECK_WORKFLOWS,
            &["ci.yml", "semantic-pr-title.yml"]
        );
        assert_eq!(SEMANTIC_PR_TITLE_INPUT, "pr_title");
        assert!(
            codex_workflow.contains("actions: write"),
            "Codex workflow needs Actions write permission to dispatch required checks"
        );
        assert!(ci_workflow.contains("workflow_dispatch:"));
        assert!(semantic_workflow.contains("workflow_dispatch:"));
        assert!(semantic_workflow.contains("pr_title:"));
        assert!(semantic_workflow.contains("github.event.pull_request.title || inputs.pr_title"));
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
            "Let Codex completion leave active set",
            "TRIGGER_SOURCE: ${{ inputs.trigger_source || '' }}",
            "codex_completion",
            "waiting for completed Codex run to leave the active run list",
            "refreshing once after black-factor top-up settles",
            "agentic-system-audit",
            "--strict",
            "gh workflow run codex-forge-loop.yml",
            "local ChatGPT auth",
            "dispatch=codex_forge_loop",
            "skipped_open_pr",
            "skipped_pr_pressure",
            "skipped_active_codex",
            "RUN_HISTORY_LIMIT: '3000'",
            "--limit \"${RUN_HISTORY_LIMIT}\"",
            "number,title,state,mergedAt,statusCheckRollup,url",
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
        let hooks = fs::read_to_string(
            root.join(".codex/archive/lifecycle-hooks-20260703T024950Z/hooks.json.md"),
        )
        .expect("read archived hooks");
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
            assert!(
                hooks.contains(required),
                "archived hooks missing {required}"
            );
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

        assert_eq!(report.checked_targets, 10);
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
            "Codex Forge Loop",
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
    fn runner_retarget_workflow_installs_tracked_script_without_tmp_lock() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let workflow = fs::read_to_string(root.join(".github/workflows/runner-retarget.yml"))
            .expect("read runner retarget workflow");
        let script = fs::read_to_string(root.join("scripts/retarget-local-runner-services.sh"))
            .expect("read tracked runner retarget script");

        assert!(
            workflow.contains("actions/checkout"),
            "retarget workflow must checkout the tracked retarget script"
        );
        assert!(
            workflow.contains(
                "sudo -n install -m 0755 scripts/retarget-local-runner-services.sh /usr/local/sbin/flexnetos-runner-retarget.sh"
            ),
            "retarget workflow must install the tracked retarget script"
        );
        assert!(
            !workflow.contains("cat > /tmp/flexnetos-runner-retarget.sh"),
            "retarget workflow must not write a fixed /tmp path"
        );
        for required in [
            "repo=/home/flexnetos/FlexNetOS/src/flexnetos_runner",
            "User=flexnetos",
            "CODEX_HOME=/home/flexnetos/.codex",
            "GH_CONFIG_DIR=/home/flexnetos/.config/gh",
            "systemctl restart",
        ] {
            assert!(
                script.contains(required),
                "retarget script missing {required}"
            );
        }
    }

    #[test]
    fn portable_runner_installer_dry_runs_user_and_system_units_from_prefix() {
        if cfg!(windows) {
            return;
        }

        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let script = root.join("scripts/install-runner-services.sh");
        let prefix = "/tmp/fxrun-portable-prefix";
        let portable_home = "/tmp/fxrun-portable-home";
        let portable_codex_home = "/tmp/fxrun-portable-auth/codex";
        let portable_gh_config_dir = "/tmp/fxrun-portable-auth/gh";
        let portable_codex_bin_dir = "/tmp/fxrun-portable-auth/bin";
        let ambient_runtime_dir = root.join("_work/fake-ci-runtime");
        let ambient_dbus_address = format!("unix:path={}/bus", ambient_runtime_dir.display());
        let ambient_xdg_config_home = root.join("_work/fake-ci-config");

        let user_output = std::process::Command::new("bash")
            .arg(&script)
            .arg("--prefix")
            .arg(prefix)
            .arg("--mode")
            .arg("user")
            .arg("--dry-run")
            .env("HOME", portable_home)
            .env("CODEX_HOME", portable_codex_home)
            .env("GH_CONFIG_DIR", portable_gh_config_dir)
            .env("FXRUN_RUNNER_CODEX_BIN_DIR", portable_codex_bin_dir)
            .env("XDG_RUNTIME_DIR", &ambient_runtime_dir)
            .env("XDG_CONFIG_HOME", &ambient_xdg_config_home)
            .env("DBUS_SESSION_BUS_ADDRESS", &ambient_dbus_address)
            .output()
            .expect("dry-run user installer");
        assert!(
            user_output.status.success(),
            "user dry-run failed: {}",
            String::from_utf8_lossy(&user_output.stderr)
        );
        let user_stdout = String::from_utf8_lossy(&user_output.stdout);
        assert!(user_stdout.contains(".config/systemd/user/flexnetos-runner@.service"));
        assert!(user_stdout.contains(
            "ExecStart=/tmp/fxrun-portable-prefix/_work/repos/actions-runner-%i/flexnetos-runner-entrypoint.sh"
        ));
        assert!(user_stdout
            .contains("WorkingDirectory=/tmp/fxrun-portable-prefix/_work/repos/actions-runner-%i"));
        assert!(
            user_stdout.contains(
                "Environment=RUNNER_WORKSPACE=/tmp/fxrun-portable-prefix/_work/actions-runner-%i-work"
            ),
            "user unit must keep RUNNER_WORKSPACE under the prefix"
        );
        assert!(user_stdout.contains("Environment=CODEX_HOME=/tmp/fxrun-portable-auth/codex"));
        assert!(user_stdout.contains("Environment=GH_CONFIG_DIR=/tmp/fxrun-portable-auth/gh"));
        assert!(user_stdout.contains(
            "Environment=GIT_CONFIG_GLOBAL=/tmp/fxrun-portable-prefix/_work/runner-home-%i/.gitconfig"
        ));
        assert!(user_stdout
            .contains("systemctl --user enable --now flexnetos-runner@01 flexnetos-runner@02"));
        assert!(!user_stdout.contains("sudo systemctl"));
        assert!(!user_stdout.contains("/home/drdave"));
        assert!(!user_stdout.contains("/home/flexnetos/FlexNetOS/src/flexnetos_runner"));

        let system_output = std::process::Command::new("bash")
            .arg(&script)
            .arg("--prefix")
            .arg(prefix)
            .arg("--mode")
            .arg("system")
            .arg("--dry-run")
            .env("HOME", portable_home)
            .env("CODEX_HOME", portable_codex_home)
            .env("GH_CONFIG_DIR", portable_gh_config_dir)
            .env("FXRUN_RUNNER_CODEX_BIN_DIR", portable_codex_bin_dir)
            .env("XDG_RUNTIME_DIR", &ambient_runtime_dir)
            .env("XDG_CONFIG_HOME", &ambient_xdg_config_home)
            .env("DBUS_SESSION_BUS_ADDRESS", &ambient_dbus_address)
            .output()
            .expect("dry-run system installer");
        assert!(
            system_output.status.success(),
            "system dry-run failed: {}",
            String::from_utf8_lossy(&system_output.stderr)
        );
        let system_stdout = String::from_utf8_lossy(&system_output.stdout);
        assert!(system_stdout.contains("/etc/systemd/system/flexnetos-runner@.service"));
        assert!(system_stdout.contains("User=flexnetos"));
        assert!(system_stdout.contains(
            "ExecStart=/tmp/fxrun-portable-prefix/_work/repos/actions-runner-%i/flexnetos-runner-entrypoint.sh"
        ));
        assert!(
            system_stdout.contains(
                "Environment=RUNNER_WORKSPACE=/tmp/fxrun-portable-prefix/_work/actions-runner-%i-work"
            ),
            "system unit must keep RUNNER_WORKSPACE under the prefix"
        );
        assert!(system_stdout.contains("Environment=CODEX_HOME=/tmp/fxrun-portable-auth/codex"));
        assert!(system_stdout.contains("Environment=GH_CONFIG_DIR=/tmp/fxrun-portable-auth/gh"));
        assert!(system_stdout.contains(
            "Environment=GIT_CONFIG_GLOBAL=/tmp/fxrun-portable-prefix/_work/runner-home-%i/.gitconfig"
        ));
        assert!(system_stdout
            .contains("systemctl enable --now flexnetos-runner@01 flexnetos-runner@02"));
        assert!(!system_stdout.contains("/home/drdave"));
        assert!(!system_stdout.contains("/home/flexnetos/FlexNetOS/src/flexnetos_runner"));
    }

    #[test]
    fn portable_runner_installer_keeps_runner_state_and_path_under_prefix() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let script = fs::read_to_string(root.join("scripts/install-runner-services.sh"))
            .expect("read portable runner installer");

        for required in [
            "_work/repos/actions-runner-01",
            "_work/repos/actions-runner-02",
            "_work/actions-runner-01-work",
            "_work/actions-runner-02-work",
            "_work/runner-home-01",
            "_work/runner-home-02",
            "CODEX_HOME",
            "GH_CONFIG_DIR",
            "GIT_CONFIG_GLOBAL",
            "RUNNER_WORKSPACE",
            "loginctl enable-linger",
            "--enable-linger",
        ] {
            assert!(script.contains(required), "installer missing {required}");
        }
        assert!(
            !script.contains("/home/drdave"),
            "portable installer must not reference the old host"
        );
        assert!(
            !script.contains("/home/flexnetos/FlexNetOS/src/flexnetos_runner"),
            "portable installer must not require the current checkout path"
        );
    }

    #[test]
    fn portable_runner_active_surfaces_do_not_require_fixed_checkout_paths() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");

        for path in [
            "scripts/install-runner-services.sh",
            "scripts/eval-runners.sh",
            ".github/workflows/ci.yml",
            ".github/workflows/runner-sustain.yml",
            ".github/workflows/runner-smoke.yml",
        ] {
            let contents = fs::read_to_string(root.join(path))
                .unwrap_or_else(|err| panic!("read portable runner active surface {path}: {err}"));
            assert!(
                !contents.contains("/home/drdave"),
                "{path} must not require the old host"
            );
            assert!(
                !contents.contains("/home/flexnetos/FlexNetOS/src/flexnetos_runner"),
                "{path} must not require the current fixed source checkout"
            );
        }
    }

    #[test]
    fn runner_queue_role_docs_and_collector_are_guarded() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let target = fs::read_to_string(root.join("docs/forge-loop/kclaw0-runner-flow-target.md"))
            .expect("read kclaw0 runner target");
        let script = fs::read_to_string(root.join("scripts/collect-runner-queue-evidence.sh"))
            .expect("read queue collector");
        let source = fs::read_to_string(root.join("crates/runner-cli/src/forge_loop.rs"))
            .expect("read forge loop source");

        for required in [
            "Runner queue role audit",
            "self-hosted",
            "linux",
            "x64",
            "local",
            "flexnetos",
            "controller role",
            "queued local-label jobs",
            "queued nonlocal jobs",
            "every non-archived `FlexNetOS/*` repo by default",
            "reducing the default FlexNetOS org scope is not completion evidence",
            "runner-queue-audit --repo-jobs-json",
            "--max-queued-local-jobs",
        ] {
            assert!(
                target.contains(required),
                "runner target missing {required}"
            );
        }
        for required in [
            "gh repo list",
            "gh run list",
            "--status \"$status\"",
            "gh api --paginate",
            "runner-queue-audit",
            "ORGS=(\"FlexNetOS\")",
            "REPO_LIMIT=\"${FXRUN_QUEUE_REPO_LIMIT:-1000}\"",
        ] {
            assert!(
                script.contains(required),
                "queue collector missing {required}"
            );
        }
        assert!(source.contains("runner_queue_audit"));
        assert!(source.contains("queue role audit"));
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
            "- Codex Forge Loop",
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
            "Codex Forge Loop",
            "required local checks need the runner lane",
            "runner-pressure.env",
            "pending/failed PR-local checks or required main-branch local checks own the runner lane",
            "required_run_pressure",
            "dispatching Runner Sustain lane",
            "runner-flow-audit",
            "--strict",
            "runner-black-factor-audit",
            "createdAt,updatedAt,event,displayTitle",
            "RUN_HISTORY_LIMIT: '3000'",
            "--limit \"${RUN_HISTORY_LIMIT}\"",
            "number,title,state,mergedAt,statusCheckRollup,url",
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
            target.contains("pending or failed PR-local checks or required main-branch local checks make the watch record a non-strict audit and stay green")
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
