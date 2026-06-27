use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_CODEX: &str = "/home/drdave/Desktop/meta/.toolchains/codex/bin/codex";
const DEFAULT_ARTIFACT_ROOT: &str = "_work/forge-loop";
const MAX_EVAL_RETRY_COUNT: u8 = 10;

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
    /// Fail when exported forge-loop upgrades are still documented as queued/backlog work.
    DocsDrift(DocsDriftArgs),
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
pub struct DocsDriftArgs {
    /// Workspace root to scan.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
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
pub struct CycleManifest {
    pub goal: String,
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
        ForgeLoopCommand::DocsDrift(args) => docs_drift(args),
    }
}

fn run(args: RunArgs) -> Result<()> {
    let cycle_dir = args.out.join(timestamp_label()?);
    fs::create_dir_all(&cycle_dir)
        .with_context(|| format!("create forge-loop artifact dir {}", cycle_dir.display()))?;
    fs::write(
        cycle_dir.join("cycle-manifest.json"),
        serde_json::to_string_pretty(&cycle_manifest(&args))?,
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
        "strict_upgrade_only": true
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
        "strict_upgrade_only": true
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
        println!("  research sources   :");
        for source in research_sources() {
            println!("    - {} ({})", source.id, source.url);
        }
    }
    Ok(())
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
            prompt,
        ],
    }
}

pub fn research_sources() -> Vec<ResearchSource> {
    vec![
        ResearchSource { id: "openai-codex", url: "https://github.com/openai/codex", purpose: "Codex Rust CLI behavior, noninteractive execution, JSONL, and upstream issues" },
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
    format!(
        "Run a Codex TDD forge-loop cycle for this Rust repo. Goal: {goal}. Required phases: write/verify a red test first, implement the smallest passing change, run fmt/clippy/tests/audit, evaluate the run, research one reliability/accuracy/speed improvement, and if a self-upgrade is warranted commit, push, open a PR, and {}. Strict upgrade only: no downgrades or removals without installed replacement and parity proof.",
        if auto_merge { "auto-merge once green when repository settings allow" } else { "leave the PR ready for review" }
    )
}

fn cycle_manifest(args: &RunArgs) -> CycleManifest {
    CycleManifest {
        goal: args.goal.clone(),
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
    fn cycle_manifest_records_once_strict_phase_contract() {
        let manifest = cycle_manifest(&RunArgs {
            goal: "cycle 05 reliability upgrade".into(),
            out: PathBuf::from("_work/forge-loop"),
            dry_run: true,
            auto_merge: true,
            once: true,
        });

        assert_eq!(manifest.goal, "cycle 05 reliability upgrade");
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
